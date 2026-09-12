//! Selection-only receive requests; results remain current original scope data.
use super::{Context, Error, transport};
use crate::receive_service::{ReceiveFile, ReceiveView};
use hagency_core::{attachments::AttachmentPage, replies::matrix_event, tasks::clock};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

pub(crate) const NAMES: [&str; 2] = ["list_received_files", "receive_file"];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Page {
    #[serde(default)]
    pub after: u64,
    #[serde(default = "page_limit")]
    pub limit: usize,
}
fn page_limit() -> usize {
    16
}
impl Page {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        clock(self.after).map_err(|_| Error::Invalid)?;
        if !(1..=16).contains(&self.limit) {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub(crate) fn validate_response(&self, page: &AttachmentPage) -> Result<(), Error> {
        if page.items.len() > self.limit
            || serde_json::to_vec(page).map_err(|_| Error::Response)?.len() > 16 * 1024
        {
            return Err(Error::Response);
        }
        let mut last = self.after;
        let mut ids = std::collections::BTreeSet::new();
        for item in &page.items {
            clock(item.sequence).map_err(|_| Error::Response)?;
            matrix_event(&item.event_id).map_err(|_| Error::Response)?;
            item.metadata.validate().map_err(|_| Error::Response)?;
            if item.sequence <= last || !ids.insert(&item.event_id) {
                return Err(Error::Response);
            }
            last = item.sequence;
        }
        if page
            .next
            .is_some_and(|next| page.items.is_empty() || next != last)
        {
            return Err(Error::Response);
        }
        Ok(())
    }
}
pub(crate) enum Command {
    Receive(ReceiveFile),
    List(Page),
}
impl Command {
    pub(crate) fn parse(name: &str, args: Value) -> Result<Self, Error> {
        match name {
            "receive_file" => {
                let input: ReceiveFile =
                    serde_json::from_value(args).map_err(|_| Error::Invalid)?;
                input.validate().map_err(|_| Error::Invalid)?;
                Ok(Self::Receive(input))
            }
            "list_received_files" => {
                let page: Page = serde_json::from_value(args).map_err(|_| Error::Invalid)?;
                page.validate()?;
                Ok(Self::List(page))
            }
            _ => Err(Error::Invalid),
        }
    }
    pub(super) fn validate(&self, context: &Context) -> Result<(), Error> {
        if !context.receive_tools() {
            return Err(Error::Invalid);
        }
        match self {
            Self::Receive(input) => input.validate().map_err(|_| Error::Invalid),
            Self::List(page) => page.validate(),
        }
    }
    pub(super) fn mutates(&self) -> bool {
        matches!(self, Self::Receive(_))
    }
    pub(super) fn response_limit(&self) -> usize {
        if self.mutates() { 4096 } else { 16 * 1024 }
    }
    pub(super) fn wire(&self) -> Result<(String, Vec<u8>, &'static str), Error> {
        Ok(match self {
            Self::Receive(input) => (
                "/api/native/v1/runner/received-files".into(),
                serde_json::to_vec(input).map_err(|_| Error::Invalid)?,
                "POST",
            ),
            Self::List(page) => (
                format!(
                    "/api/native/v1/runner/received-files?after={}&limit={}",
                    page.after, page.limit
                ),
                vec![],
                "GET",
            ),
        })
    }
}
pub(crate) async fn run(
    context: &Context,
    command: &Command,
    deadline: Duration,
) -> Result<Value, Error> {
    let bytes =
        transport::request(context, transport::Operation::Received(command), deadline).await?;
    let failure = if command.mutates() {
        Error::Unknown
    } else {
        Error::Response
    };
    let value = crate::mcp::json::json(&bytes).map_err(|_| failure)?;
    match command {
        Command::Receive(input) => {
            let received: ReceiveView =
                serde_json::from_value(value.clone()).map_err(|_| failure)?;
            received.validate().map_err(|_| failure)?;
            if received.event_id != input.event_id {
                return Err(failure);
            }
        }
        Command::List(request) => {
            let page: AttachmentPage =
                serde_json::from_value(value.clone()).map_err(|_| failure)?;
            request.validate_response(&page)?;
        }
    }
    Ok(value)
}
