//! The console account surface without readiness (MA-S3a): a bounded
//! observation of the host's credential namespaces plus the three mutations
//! under one finite scope. ADR-114's opacity is unchanged — no readiness, no
//! login, no auth-file read — so the wire item is `AccountRow`, never
//! `AccountChoice`: the CLI's DTO carries `authentication`/`quota`, which
//! would read as a readiness answer today ("unknown"/null) and would break
//! the exact-key validator when MA-S3b lands. Reads stay scope-free like
//! every other pure read; only the mutations consult the authority, and the
//! missing-scope word (`account_scope_required`) is served before any store
//! job, so a read-only session changes nothing.
//!
//! Authority asymmetry (ADR-108 amendment): enrolment binds a concrete
//! non-Clone `AccountEnrollmentCommand` through the SESSION's
//! `AccountEnrollmentAccess`; reserve+materialize and retire are plain
//! `&mut DomainRepository` acts whose console authority is the session-scope
//! boolean. The unknown window is wider than the offline verb's (2 s reply
//! bound vs the store's 5 s preparation deadline): a slow-but-successful
//! prepare surfaces as `outcome_unknown`, and `materialize_account`'s
//! `'uncertain'`-before-`mkdir` ordering keeps the row inspectable either
//! way — this route never invents an outcome.
use super::{Error, Session, body, console, failed, recheck, usage::query};
use crate::{refusal, resources::domain};
use hagency_core::project::identifier;
use hagency_store::AccountChoice;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub(super) fn router() -> Router {
    Router::new()
        .push(Router::with_path("accounts").get(list).post(prepare))
        .push(Router::with_path("accounts/{id}").get(single))
        .push(Router::with_path("accounts/{id}/retire").post(retire))
        .push(Router::with_path("accounts/{id}/enrollment").post(enrollment))
}

/// The browser DTO: exactly six keys (id, ordinal, state, revision,
/// profile, readiness). None of the identity triple (`namespace_identity`,
/// `identity_tuple`, `seat_id`) and no preset association crosses the wire.
/// `readiness` is MA-S1's observed fact read at serve time (the latest
/// observed, unexpired mode, else `unknown`); the console computes nothing
/// and serves no fix pointer — the login is the operator's own host act
/// (D-ADR114 observe). No credential home path, no credential-present file
/// answer, no token-shaped byte, no probe output ever crosses.
#[derive(Serialize)]
struct AccountRow {
    id: String,
    ordinal: u8,
    state: hagency_store::AccountState,
    revision: String,
    profile: String,
    readiness: &'static str,
}
impl AccountRow {
    /// Serve-time read of the recorded fact: a read never writes and never
    /// promotes; expired and `uncertain` degrade to `unknown` inside the
    /// store's own rule.
    async fn from_choice(
        store: &hagency_store::DomainStore,
        choice: AccountChoice,
        now: u64,
    ) -> Result<Self, hagency_store::Error> {
        let answer = store.account_readiness(choice.id.clone(), now).await?;
        let readiness = match answer.mode {
            hagency_store::AccountReadinessMode::Subscription => "subscription",
            hagency_store::AccountReadinessMode::ApiKey => "api_key",
            hagency_store::AccountReadinessMode::Unknown => "unknown",
        };
        Ok(Self {
            id: choice.id,
            ordinal: choice.ordinal,
            state: choice.state,
            revision: choice.revision,
            profile: choice.profile,
            readiness,
        })
    }
}
fn failure(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_account_command"),
        hagency_store::Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        hagency_store::Error::State => (StatusCode::CONFLICT, "account_state_conflict"),
        hagency_store::Error::Conflict => (StatusCode::CONFLICT, "account_revision_conflict"),
        hagency_store::Error::LocalAuthority => {
            (StatusCode::UNAUTHORIZED, "console_access_required")
        }
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "native_unavailable"),
    };
    refusal(res, status, code);
}
fn session(depot: &Depot) -> Result<&Session, Error> {
    depot
        .get_typed::<Session>()
        .map_err(|_| Error::Unauthorized)
}
/// The finite account scope, consulted before any store job so a read-only
/// session never mutates a row (its refusal is the scope word, not a store
/// error).
fn account_scope(depot: &Depot) -> Result<(), Error> {
    match console(depot)?
        .0
        .authority
        .can_manage_accounts(session(depot)?)
    {
        Ok(true) => Ok(()),
        Ok(false) => Err(Error::AccountForbidden),
        Err(error) => Err(error),
    }
}
fn account_id(req: &Request) -> Result<String, Error> {
    let id = req.param::<String>("id").ok_or(Error::Invalid)?;
    identifier(&id, 128).map_err(|_| Error::Invalid)?;
    Ok(id)
}
async fn one_row(
    store: &hagency_store::DomainStore,
    id: &str,
    now: u64,
) -> Result<AccountRow, hagency_store::Error> {
    let Some(choice) = store
        .account_choices()
        .await?
        .into_iter()
        .find(|choice| choice.id == id)
    else {
        return Err(hagency_store::Error::NotFound);
    };
    AccountRow::from_choice(store, choice, now).await
}
fn statement_time() -> Result<u64, Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or(Error::Unavailable)
}
#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.account_choices().await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(choices) => {
            let at = match statement_time() {
                Ok(at) => at,
                Err(error) => {
                    failed(res, error);
                    return;
                }
            };
            let mut rows: Vec<AccountRow> = Vec::with_capacity(choices.len());
            for choice in choices {
                match AccountRow::from_choice(&store, choice, at).await {
                    Ok(row) => rows.push(row),
                    Err(error) => {
                        failure(res, error);
                        return;
                    }
                }
            }
            super::resources::bounded(
                res,
                &serde_json::json!({"at_ms":at,"accounts":rows,"next_after":null}),
            );
        }
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn single(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let id = match account_id(req).map_err(|error| {
        failed(res, error);
    }) {
        Ok(id) => id,
        Err(()) => return,
    };
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let at = match statement_time() {
        Ok(at) => at,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    let result = one_row(&store, &id, at).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(row) => super::resources::bounded(res, &serde_json::json!({"account":row})),
        Err(error) => failure(res, error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Prepare {
    profile: String,
}
/// Reserve + materialize as one console act: the store's one-shot ordering
/// (the row goes `'uncertain'` before `mkdir`) is the store's, unchanged.
#[handler]
async fn prepare(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if let Err(error) = account_scope(depot) {
        failed(res, error);
        return;
    }
    let input: Prepare = match serde_json::from_slice(&body(req, 256).await.unwrap_or_default())
        .map_err(|_| Error::Invalid)
    {
        Ok(input) => input,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    if input.profile.len() > 128 {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = async {
        let reserved = store.reserve_account(input.profile).await?;
        let choice = store.materialize_account(reserved.id).await?;
        Ok::<_, hagency_store::Error>(choice)
    }
    .await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    let at = match statement_time() {
        Ok(at) => at,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    match result {
        Ok(choice) => match AccountRow::from_choice(&store, choice, at).await {
            Ok(row) => super::resources::bounded(res, &serde_json::json!({"account":row})),
            Err(error) => failure(res, error),
        },
        Err(error) => failure(res, error),
    }
}
/// Retire takes no body and no expected revision: the store fences the
/// binding and unpublishes the account's resources in one job.
#[handler]
async fn retire(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if let Err(error) = account_scope(depot) {
        failed(res, error);
        return;
    }
    let id = match account_id(req).map_err(|error| {
        failed(res, error);
    }) {
        Ok(id) => id,
        Err(()) => return,
    };
    if req.uri().query().is_some() || !body(req, 1).await.unwrap_or_default().is_empty() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.retire_account(id).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    let at = match statement_time() {
        Ok(at) => at,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    match result {
        Ok(choice) => match AccountRow::from_choice(&store, choice, at).await {
            Ok(row) => super::resources::bounded(res, &serde_json::json!({"account":row})),
            Err(error) => failure(res, error),
        },
        Err(error) => failure(res, error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Enrollment {
    model: String,
    reasoning: Option<String>,
    expected_revision: String,
}
/// The credential-binding act: the only mutation that takes an expected
/// revision, and the only one whose authority is a concrete command built
/// from the session's access — browser payloads cannot supply it.
#[handler]
async fn enrollment(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if let Err(error) = account_scope(depot) {
        failed(res, error);
        return;
    }
    let id = match account_id(req).map_err(|error| {
        failed(res, error);
    }) {
        Ok(id) => id,
        Err(()) => return,
    };
    let input: Enrollment = match serde_json::from_slice(&body(req, 1024).await.unwrap_or_default())
        .map_err(|_| Error::Invalid)
    {
        Ok(input) => input,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    if input.model.len() > 256 || input.expected_revision.len() > 128 {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    // Phase 1 (store): the handle the command binds is a store read.
    let read = store.managed_account(id.clone()).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    let managed = match read {
        Ok(managed) => managed,
        Err(error) => {
            failure(res, error);
            return;
        }
    };
    // Phase 2 (console): the consuming command is built from the SESSION's
    // bound access — browser payloads cannot supply authority.
    let command = match console(depot).and_then(|c| {
        c.0.authority.account(
            session(depot)?,
            &managed,
            super::authority::AccountEnrollmentInput {
                revision: input.expected_revision,
                model: input.model,
                reasoning: input.reasoning,
                ceiling: None,
                deadline: Instant::now() + Duration::from_secs(5),
            },
        )
    }) {
        Ok(command) => command,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    // Phase 3 (store): the write and the read-back.
    let result = async {
        store.enroll_account_resource(command).await?;
        let at = statement_time().map_err(|_| hagency_store::Error::Unavailable)?;
        one_row(&store, &id, at).await
    }
    .await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(row) => super::resources::bounded(res, &serde_json::json!({"account":row})),
        Err(error) => failure(res, error),
    }
}
