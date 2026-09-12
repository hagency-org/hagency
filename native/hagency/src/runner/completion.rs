//! The only post-fence operation is an exact already-committed finish receipt.
//! All other runner paths still require the ordinary current-capability hoop.
use super::*;
use hagency_core::completions::CompleteTaskWithReply;
pub(super) fn router() -> Router {
    Router::with_path("complete-task-with-reply").post(finish)
}
#[handler]
async fn finish(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if !local_authority(req, depot, res) {
        return;
    }
    let Some(cap) = credential(req) else {
        refusal(res, StatusCode::UNAUTHORIZED, "runner_auth_required");
        return;
    };
    let Some(store) = resources::domain(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<CompleteTaskWithReply>(req, depot, res).await else {
        return;
    };
    match store
        .runner_command(cap, RunnerCommand::CompleteTaskWithReply(input))
        .await
    {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
