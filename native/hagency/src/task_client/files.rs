//! Closed selection-only operations over the original task-client context.
use super::{Context, Error, transport};
use crate::file_service::{FileView, SendFile};
use hagency_core::project::identifier;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

pub(crate) const NAMES: [&str; 2] = ["send_file", "get_file_delivery"];
pub(crate) enum Command {
    Send(SendFile),
    Inspect { delivery_id: String },
}
impl Command {
    pub(crate) fn parse(name: &str, args: Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Inspect {
            delivery_id: String,
        }
        match name {
            "send_file" => {
                let input: SendFile = serde_json::from_value(args).map_err(|_| Error::Invalid)?;
                input.validate().map_err(|_| Error::Invalid)?;
                Ok(Self::Send(input))
            }
            "get_file_delivery" => {
                let Inspect { delivery_id } =
                    serde_json::from_value(args).map_err(|_| Error::Invalid)?;
                identifier(&delivery_id, 128).map_err(|_| Error::Invalid)?;
                Ok(Self::Inspect { delivery_id })
            }
            _ => Err(Error::Invalid),
        }
    }
    pub(super) fn validate(&self, context: &Context) -> Result<(), Error> {
        if !context.file_tools() {
            return Err(Error::Invalid);
        }
        match self {
            Self::Send(input) => input.validate().map_err(|_| Error::Invalid),
            Self::Inspect { delivery_id } => {
                identifier(delivery_id, 128).map_err(|_| Error::Invalid)
            }
        }
    }
    pub(super) fn mutates(&self) -> bool {
        matches!(self, Self::Send(_))
    }
    pub(super) fn wire(&self) -> Result<(String, Vec<u8>, &'static str), Error> {
        Ok(match self {
            Self::Send(input) => (
                "/api/native/v1/runner/file-deliveries".into(),
                serde_json::to_vec(input).map_err(|_| Error::Invalid)?,
                "POST",
            ),
            Self::Inspect { delivery_id } => (
                format!("/api/native/v1/runner/file-deliveries/{delivery_id}"),
                Vec::new(),
                "GET",
            ),
        })
    }
}
pub(crate) async fn run(
    context: &Context,
    command: &Command,
    deadline: Duration,
) -> Result<FileView, Error> {
    let bytes = transport::request(context, transport::Operation::Files(command), deadline).await?;
    let failure = if command.mutates() {
        Error::Unknown
    } else {
        Error::Response
    };
    let value = crate::mcp::json::json(&bytes).map_err(|_| failure)?;
    let view: FileView = serde_json::from_value(value).map_err(|_| failure)?;
    view.validate().map_err(|_| failure)?;
    if let Command::Inspect { delivery_id } = command
        && view.delivery_id != *delivery_id
    {
        return Err(failure);
    }
    Ok(view)
}
