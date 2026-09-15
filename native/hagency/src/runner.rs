//! Private runner task surface. Host dispatch/process and operator APIs are separate.
use crate::{local_authority, refusal, resources};
use hagency_core::{
    conversations::{ConversationChange, ConversationRequest},
    peers::PeerSend,
    project::identifier,
    task_intents::Delegation,
    tasks::{RunnerCapability, RunnerCommand, TaskMutation},
};
use hagency_store::{DomainStore, Error};
use salvo::prelude::*;
mod completion;
mod files;
mod received;
mod replies;
mod workflows;

#[derive(Clone)]
struct Context {
    store: DomainStore,
    cap: RunnerCapability,
}
pub(super) fn router() -> Router {
    Router::with_path("api/native/v1/runner")
        .push(completion::router())
        .push(files::historical())
        .push(
            Router::new()
                .hoop(authenticate)
                .push(files::current())
                .push(received::router())
                .push(Router::with_path("tasks").get(list_tasks))
                .push(Router::with_path("delegations").post(delegate))
                .push(Router::with_path("conversations").post(open_conversation))
                .push(Router::with_path("conversations/{id}").get(conversation))
                .push(Router::with_path("conversations/{id}/operations").post(change_conversation))
                .push(Router::with_path("tasks/{id}").get(get_task))
                .push(Router::with_path("tasks/{id}/comments").get(comments))
                .push(Router::with_path("tasks/{id}/operations").post(mutate))
                .push(Router::with_path("inbox").get(inbox))
                .push(Router::with_path("peer-messages").post(send_peer))
                .push(Router::with_path("peer-inbox").get(peer_inbox))
                .push(workflows::router())
                .push(replies::router()),
        )
}
#[handler]
async fn change_conversation(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(change) = resources::body::<ConversationChange>(req, depot, res).await else {
        return;
    };
    let result = async {
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::ChangeConversation {
                    id: task_id(req)?,
                    change,
                },
            )
            .await
    }
    .await;
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn send_peer(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<PeerSend>(req, depot, res).await else {
        return;
    };
    match c
        .store
        .runner_command(c.cap, RunnerCommand::SendPeer(input))
        .await
    {
        Ok(value) => res.render(Json(value)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn peer_inbox(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        let (after, limit) = page(req)?;
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::PeerInbox {
                    after: sequence(&after)?,
                    limit,
                },
            )
            .await
    }
    .await;
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
fn single_header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    if req.headers().get_all(name).iter().count() != 1 {
        return None;
    }
    req.headers().get(name)?.to_str().ok()
}
#[handler]
async fn open_conversation(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<ConversationRequest>(req, depot, res).await else {
        return;
    };
    match c
        .store
        .runner_command(c.cap, RunnerCommand::OpenConversation(input))
        .await
    {
        Ok(value) => res.render(Json(value)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn conversation(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        c.store
            .runner_command(c.cap, RunnerCommand::Conversation { id: task_id(req)? })
            .await
    }
    .await;
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn delegate(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(input) = resources::body::<Delegation>(req, depot, res).await else {
        return;
    };
    match c
        .store
        .runner_command(c.cap, RunnerCommand::Delegate(input))
        .await
    {
        Ok(value) => res.render(Json(value)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
fn credential(req: &Request) -> Option<RunnerCapability> {
    if [
        "token",
        "access_token",
        "secret",
        "capability",
        "runner_id",
        "dispatch_id",
        "fence",
    ]
    .iter()
    .any(|key| req.query::<String>(key).is_some())
    {
        return None;
    }
    let secret = single_header(req, "authorization")?.strip_prefix("Bearer ")?;
    if secret.len() != 64 || !secret.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let dispatch_id = single_header(req, "x-hagency-dispatch")?;
    let runner_id = single_header(req, "x-hagency-runner")?;
    identifier(dispatch_id, 128).ok()?;
    identifier(runner_id, 128).ok()?;
    let fence = single_header(req, "x-hagency-fence")?.parse::<u64>().ok()?;
    if fence == 0 || fence > hagency_core::JSON_SAFE_MAX {
        return None;
    }
    Some(RunnerCapability {
        secret: secret.into(),
        dispatch_id: dispatch_id.into(),
        runner_id: runner_id.into(),
        fence,
    })
}
fn failure(res: &mut Response, error: Error) {
    // Submodule callers (completion/replies/workflows, via `use super::*`)
    // have no store parameter here and keep today's plain code; the runner.rs
    // sites below call `attributed_failure` with the store (see the report:
    // the accepted design's 13-site count missed these four callers).
    attributed_failure(res, None, error);
}
fn attributed_failure(res: &mut Response, store: Option<&DomainStore>, error: Error) {
    let (status, code) = match error {
        Error::RunnerAuthority | Error::NotFound => (StatusCode::FORBIDDEN, "task_scope_required"),
        Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_task_operation"),
        Error::Conflict => (StatusCode::CONFLICT, "idempotency_conflict"),
        Error::State | Error::Quarantined | Error::Generation => {
            (StatusCode::CONFLICT, "state_conflict")
        }
        Error::Busy | Error::Capacity => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        // A 504 stays a 504: the command's outcome is unknown either way and
        // must be reconciled, never retried as success (ADR-053:100-104,147-148).
        // The code names WHICH side of the writer queue the expiry happened on,
        // using the store's own diagnostic bit. It grants no retry authority.
        Error::OutcomeUnknown => (
            StatusCode::GATEWAY_TIMEOUT,
            match store.and_then(DomainStore::last_unknown_dequeued) {
                Some(true) => "outcome_unknown_running",
                _ => "outcome_unknown",
            },
        ),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    };
    refusal(res, status, code);
}
#[handler]
async fn authenticate(
    req: &mut Request,
    depot: &mut Depot,
    res: &mut Response,
    ctrl: &mut FlowCtrl,
) {
    if !local_authority(req, depot, res) {
        ctrl.skip_rest();
        return;
    }
    let Some(cap) = credential(req) else {
        refusal(res, StatusCode::UNAUTHORIZED, "runner_auth_required");
        ctrl.skip_rest();
        return;
    };
    let Some(store) = resources::domain(depot, res) else {
        ctrl.skip_rest();
        return;
    };
    match store
        .runner_command(cap.clone(), RunnerCommand::Check)
        .await
    {
        Ok(_) => {
            depot.insert_typed(Context { store, cap });
        }
        Err(
            Error::RunnerAuthority
            | Error::NotFound
            | Error::Quarantined
            | Error::Generation
            | Error::State,
        ) => {
            refusal(res, StatusCode::UNAUTHORIZED, "runner_auth_required");
            ctrl.skip_rest();
        }
        Err(error) => {
            attributed_failure(res, Some(&store), error);
            ctrl.skip_rest();
        }
    }
}
fn context(depot: &Depot, res: &mut Response) -> Option<Context> {
    let value = depot.get_typed::<Context>().ok().cloned();
    if value.is_none() {
        refusal(res, StatusCode::UNAUTHORIZED, "runner_auth_required");
    }
    value
}
fn page(req: &Request) -> Result<(String, usize), Error> {
    let after = req.query::<String>("after").unwrap_or_default();
    if after.len() > 128 {
        return Err(hagency_core::InvalidInput("invalid cursor").into());
    }
    let limit = req
        .query::<String>("limit")
        .map(|n| n.parse::<usize>())
        .transpose()
        .map_err(|_| hagency_core::InvalidInput("invalid page limit"))?
        .unwrap_or(100);
    if !(1..=100).contains(&limit) {
        return Err(hagency_core::InvalidInput("page must be 1..100").into());
    }
    Ok((after, limit))
}
fn task_id(req: &Request) -> Result<String, Error> {
    let id = req
        .param::<String>("id")
        .ok_or(hagency_core::InvalidInput("task id required"))?;
    identifier(&id, 128)?;
    Ok(id)
}
fn sequence(value: &str) -> Result<u64, Error> {
    if value.is_empty() {
        return Ok(0);
    }
    let n = value
        .parse()
        .map_err(|_| hagency_core::InvalidInput("invalid sequence"))?;
    hagency_core::tasks::clock(n)?;
    Ok(n)
}
#[handler]
async fn list_tasks(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        let (after, limit) = page(req)?;
        c.store
            .runner_command(c.cap, RunnerCommand::Tasks { after, limit })
            .await
    }
    .await;
    match result {
        Ok(tasks) => res.render(Json(tasks)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn get_task(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        c.store
            .runner_command(c.cap, RunnerCommand::Task { id: task_id(req)? })
            .await
    }
    .await;
    match result {
        Ok(task) => res.render(Json(task)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn comments(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        let (after, limit) = page(req)?;
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::Comments {
                    id: task_id(req)?,
                    after: sequence(&after)?,
                    limit,
                },
            )
            .await
    }
    .await;
    match result {
        Ok(rows) => res.render(Json(rows)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn inbox(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(c) = context(depot, res) else {
        return;
    };
    let result = async {
        let (after, limit) = page(req)?;
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::Inbox {
                    after: sequence(&after)?,
                    limit,
                },
            )
            .await
    }
    .await;
    match result {
        Ok(rows) => res.render(Json(rows)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
#[handler]
async fn mutate(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Command {
        call_id: String,
        operation: TaskMutation,
    }
    let Some(c) = context(depot, res) else {
        return;
    };
    let Some(command) = resources::body::<Command>(req, depot, res).await else {
        return;
    };
    // The domain writer obtains its clock after both body reading and queueing.
    // It rechecks expiry/parking/revocation at the actual task operation.
    let result = async {
        c.store
            .runner_command(
                c.cap,
                RunnerCommand::Mutate {
                    id: task_id(req)?,
                    call_id: command.call_id,
                    operation: command.operation,
                },
            )
            .await
    }
    .await;
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => attributed_failure(res, Some(&c.store), error),
    }
}
