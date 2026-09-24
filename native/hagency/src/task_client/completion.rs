use super::{Context, Error, transport};
use hagency_core::completions::{CompleteTaskWithReply, CompletionReceipt};
use std::time::Duration;
pub(crate) async fn run(
    context: &Context,
    input: &CompleteTaskWithReply,
    deadline: Duration,
) -> Result<CompletionReceipt, Error> {
    let bytes =
        transport::request(context, transport::Operation::Complete(input), deadline).await?;
    let receipt: CompletionReceipt = serde_json::from_slice(&bytes).map_err(|_| Error::Unknown)?;
    if receipt.task_id != context.task_id() || receipt.execution_epoch == 0 {
        return Err(Error::Unknown);
    }
    Ok(receipt)
}
