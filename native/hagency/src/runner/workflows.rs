use super::*;
use hagency_core::workflows::*;

pub(super) fn router() -> Router {
    Router::with_path("graphs")
        .get(list)
        .post(create)
        .push(Router::with_path("{id}").get(read))
        .push(Router::with_path("{id}/cancel").post(cancel))
        .push(Router::with_path("{id}/results").post(report_result))
        .push(
            Router::with_path("{id}/dependencies")
                .get(dependencies)
                .post(dependency),
        )
}
#[handler]
async fn create(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<WorkflowRequest>(req, depot, res).await else {
        return;
    };
    respond(
        res,
        c.store
            .runner_command(c.cap, RunnerCommand::CreateWorkflow(input))
            .await,
    );
}
#[handler]
async fn read(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        c.store
            .runner_command(c.cap, RunnerCommand::Workflow { id: task_id(req)? })
            .await
    }
    .await;
    respond(res, result);
}
#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        let (after, limit) = page(req)?;
        c.store
            .runner_command(c.cap, RunnerCommand::Workflows { after, limit })
            .await
    }
    .await;
    respond(res, result);
}
#[handler]
async fn cancel(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<WorkflowCancel>(req, depot, res).await else {
        return;
    };
    let result = async {
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::CancelWorkflow {
                    id: task_id(req)?,
                    input,
                },
            )
            .await
    }
    .await;
    respond(res, result);
}
#[handler]
async fn report_result(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<WorkflowResultRequest>(req, depot, res).await else {
        return;
    };
    let result = async {
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::WorkflowResult {
                    id: task_id(req)?,
                    input,
                },
            )
            .await
    }
    .await;
    respond(res, result);
}
#[handler]
async fn dependencies(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        let (after, limit) = page(req)?;
        let limit = if req.query::<String>("limit").is_none() {
            32
        } else {
            limit
        };
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::WorkflowDependencies {
                    id: task_id(req)?,
                    after: sequence(&after)?,
                    limit,
                },
            )
            .await
    }
    .await;
    respond(res, result);
}
#[handler]
async fn dependency(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<DependencyRequest>(req, depot, res).await else {
        return;
    };
    let result = async {
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::WorkflowDependency {
                    id: task_id(req)?,
                    node_id: input.node_id,
                },
            )
            .await
    }
    .await;
    respond(res, result);
}
fn respond(res: &mut Response, result: Result<serde_json::Value, Error>) {
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
