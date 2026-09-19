use super::*;
use hagency_core::replies::FinalReply;

pub(super) fn router() -> Router {
    Router::with_path("final-replies")
        .post(submit)
        .push(Router::with_path("{id}").get(read))
}
#[handler]
async fn submit(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else { return };
    let Some(input) = resources::body::<FinalReply>(req, depot, res).await else {
        return;
    };
    match c
        .store
        .runner_command(c.cap, RunnerCommand::SubmitFinalReply(input))
        .await
    {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn read(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else { return };
    let result = async {
        c.store
            .runner_command(c.cap, RunnerCommand::FinalReply { id: task_id(req)? })
            .await
    }
    .await;
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
