//! The only post-fence operations are an exact already-committed finish
//! receipt and fenced late output. All other runner paths still require
//! the ordinary current-capability hoop.
use super::*;
use hagency_core::completions::CompleteTaskWithReply;
pub(super) fn router() -> Router {
    Router::new()
        .push(Router::with_path("complete-task-with-reply").post(finish))
        .push(Router::with_path("late-output").post(late))
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
/// G12 (ADR-146, [REQ-TSS-FENCE]): a runner whose dispatch has already left
/// `started` — a fence generation that survived the process, a backend
/// restart while a child lived — submits its output here. The route sits
/// OUTSIDE the `authenticate` hoop on purpose: `RunnerCommand::Check`
/// authorizes only `started` dispatches, so routing late output through it
/// would refuse the very arrival the fence requirement says MUST be
/// recorded. The store's write carries the whole authority instead: it
/// authenticates against `runner_attempts` by exact `runner_id` and secret
/// with NO clock (ADR-053 D-7), bounds the output at 32 KiB and the row
/// count at 128, and inserts `accepted=0` — fenced display-only evidence
/// that settles nothing. No dispatch state is consulted or changed.
#[handler]
async fn late(req: &mut Request, depot: &mut Depot, res: &mut Response) {
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
    let Some(output) = resources::body::<serde_json::Value>(req, depot, res).await else {
        return;
    };
    match store.record_late_output(cap, output).await {
        Ok(()) => res.render(Json(serde_json::Value::Null)),
        Err(error) => failure(res, error),
    }
}
