//! The frozen room discussion of the dispatch this helper already holds. The
//! offset is the only input: no room, agent, session or other dispatch is
//! nameable, so the read cannot reach past the runner's own authority.
use super::{Context, Error, transport};
use hagency_core::{
    messages::{DISCUSSION_PAGE, DISCUSSION_PART, DiscussionPage},
    replies::matrix_event,
    tasks::clock,
};
use std::time::Duration;

pub(crate) const NAME: &str = "read_conversation";

/// A page the service returned is only usable when it is the page that was
/// asked for: in order, bounded, and consistent about what remains.
fn validate(offset: u64, page: &DiscussionPage) -> Result<(), Error> {
    if page.messages.len() > DISCUSSION_PAGE
        || page.total_parts > hagency_core::JSON_SAFE_MAX
        || u64::try_from(page.messages.len()).map_err(|_| Error::Response)? + offset
            > page.total_parts
    {
        return Err(Error::Response);
    }
    for part in &page.messages {
        matrix_event(&part.event_id).map_err(|_| Error::Response)?;
        if part.part == 0 || part.part > part.parts || part.body.chars().count() > DISCUSSION_PART {
            return Err(Error::Response);
        }
    }
    let end = offset + page.messages.len() as u64;
    if page.next != (end < page.total_parts).then_some(end) {
        return Err(Error::Response);
    }
    Ok(())
}
pub(crate) async fn run(
    context: &Context,
    offset: u64,
    deadline: Duration,
) -> Result<DiscussionPage, Error> {
    clock(offset).map_err(|_| Error::Invalid)?;
    let bytes = transport::request(
        context,
        transport::Operation::Discussion { offset },
        deadline,
    )
    .await?;
    let page: DiscussionPage = serde_json::from_slice(&bytes).map_err(|_| Error::Response)?;
    validate(offset, &page)?;
    Ok(page)
}
