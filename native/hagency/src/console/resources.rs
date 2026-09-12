//! Safe native observations and the single explicitly scoped catalog mutation.
use super::{Error, Session, body, console, failed, recheck, usage::query};
use crate::{refusal, resources::domain};
use hagency_core::{allocation::Ceiling, project::identifier, qualification::Tier};
use hagency_store::resource_publication_revision;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub(super) fn router() -> Router {
    Router::new()
        .push(
            Router::with_path("resources")
                .get(configurations)
                .post(super::resource_configuration::create),
        )
        .push(Router::with_path("resources/{id}/budget").get(budget))
        .push(Router::with_path("resources/{id}/publication").post(publication))
}
pub(super) fn selection_query(req: &Request) -> Result<(), Error> {
    query(req, &["resource_id"], 160)?;
    if let Some(id) = req.query::<String>("resource_id") {
        identifier(&id, 128).map_err(|_| Error::Invalid)?;
    }
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResourceRow {
    id: String,
    framework: String,
    model: String,
    provider: Option<String>,
    reasoning: Option<String>,
    ceiling: Option<Ceiling>,
    published: bool,
    roles: Vec<String>,
    revision: String,
}
impl ResourceRow {
    pub(super) fn from_resource(
        resource: hagency_core::project::Resource,
    ) -> Result<Self, hagency_store::Error> {
        Ok(Self {
            id: resource.id(),
            revision: resource_publication_revision(&resource)?,
            roles: resource.eligible_roles(),
            framework: resource.framework,
            model: resource.model,
            provider: resource.provider,
            reasoning: resource.reasoning,
            ceiling: resource.ceiling,
            published: resource.published,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RoleRow {
    role: String,
    explicit_publication: Option<bool>,
    available: bool,
    cross_family: bool,
    default_tier: Option<Tier>,
}
pub(super) fn bounded(res: &mut Response, value: &impl Serialize) {
    struct Bytes(Vec<u8>);
    impl std::io::Write for Bytes {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > (64 * 1024usize).saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("bounded console response"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bytes = Bytes(Vec::new());
    if serde_json::to_writer(&mut bytes, value).is_ok() {
        res.render(Text::Json(
            String::from_utf8(bytes.0).expect("JSON is UTF-8"),
        ));
    } else {
        refusal(res, StatusCode::SERVICE_UNAVAILABLE, "native_unavailable");
    }
}
pub(super) fn failure(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_resource_command"),
        hagency_store::Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        hagency_store::Error::State => (StatusCode::CONFLICT, "resource_in_use"),
        hagency_store::Error::Conflict => (StatusCode::CONFLICT, "resource_revision_conflict"),
        hagency_store::Error::LocalAuthority => {
            (StatusCode::UNAUTHORIZED, "console_access_required")
        }
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "native_unavailable"),
    };
    refusal(res, status, code);
}
#[handler]
async fn configurations(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &["after", "limit"], 192).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let after = req.query::<String>("after").unwrap_or_default();
    let limit = match req.query::<String>("limit") {
        None => 16,
        Some(s) if s.bytes().all(|b| b.is_ascii_digit()) => s.parse::<usize>().unwrap_or(0),
        _ => 0,
    };
    if !(1..=16).contains(&limit) || (!after.is_empty() && identifier(&after, 128).is_err()) {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = async {
        let mut rows = store.resource_configurations(after, limit + 1).await?;
        let next_after = (rows.len() > limit).then(|| rows[limit - 1].id.clone());
        rows.truncate(limit);
        let resources = rows
            .into_iter()
            .map(|r| {
                let revision = resource_publication_revision(&r.config)?;
                Ok(ResourceRow {
                    id: r.id,
                    framework: r.config.framework,
                    model: r.config.model,
                    provider: r.config.provider,
                    reasoning: r.config.reasoning,
                    ceiling: r.config.ceiling,
                    published: r.config.published,
                    roles: r.config.roles,
                    revision,
                })
            })
            .collect::<Result<Vec<_>, hagency_store::Error>>()?;
        let roles = store
            .role_publications()
            .await?
            .into_iter()
            .map(serde_json::from_value::<RoleRow>)
            .collect::<Result<Vec<_>, _>>()?;
        if roles.len() != hagency_core::qualification::roles().count() {
            return Err(hagency_store::Error::Schema);
        }
        Ok::<_, hagency_store::Error>((resources, roles, next_after))
    }
    .await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok((resources, roles, next_after)) => {
            let permission = console(depot).and_then(|c| {
                c.0.authority.can_publish(
                    depot
                        .get_typed::<Session>()
                        .map_err(|_| Error::Unauthorized)?,
                )
            });
            let configure = console(depot).and_then(|c| {
                c.0.authority.can_configure(
                    depot
                        .get_typed::<Session>()
                        .map_err(|_| Error::Unauthorized)?,
                )
            });
            let configure = match configure {
                Ok(v) => v,
                Err(error) => {
                    failed(res, error);
                    return;
                }
            };
            match permission {
                Ok(publish_resource) => bounded(
                    res,
                    &serde_json::json!({"resources":resources,"roles":roles,"next_after":next_after,"permissions":{"publishResource":publish_resource,"configureResource":configure}}),
                ),
                Err(error) => failed(res, error),
            }
        }
        Err(error) => failure(res, error),
    }
}
pub(super) fn resource_id(req: &Request) -> Result<String, Error> {
    let id = req.param::<String>("id").ok_or(Error::Invalid)?;
    identifier(&id, 128).map_err(|_| Error::Invalid)?;
    Ok(id)
}
#[handler]
async fn budget(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let id = match resource_id(req).and_then(|id| query(req, &[], 0).map(|_| id)) {
        Ok(id) => id,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    // Brief 18: ONE writer job returns the commitments budget AND the draw
    // report together, so the page can never mix figures from two reads.
    // Statement time (the retained budget read has no clock parameter), and
    // the no-query rule above stays.
    // Brief 20 (E2): a CLOCK fault is `console_unavailable` (503), never
    // `busy` — `Busy` names worker saturation and invites a retry; a failed
    // clock is the console's own unavailability. `console_unavailable` is
    // not in the client's retryable set, so no reader retries on it.
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok());
    let at = match at {
        Some(at) => at,
        None => {
            failed(res, Error::Unavailable);
            return;
        }
    };
    let result = store.resource_headroom(id, at).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok((value, draw)) => {
            // The `draw` object carries the headroom figures with unknown
            // rendered as unknown (null, never zero) and the binding draw
            // named the way ADR-122's refusal names it
            // (`engagement-store.js:82-83`: measured > committed ? "measured
            // spend" : "committed allocations"; unknown measurement leaves
            // the commitment standing alone, so the binding is null).
            let binding = match draw.spent {
                Some(spent) if spent > draw.reserved => "measured spend",
                Some(_) => "committed allocations",
                None => "",
            };
            let remaining_before_ceiling = draw
                .ceiling_tokens
                .map(|ceiling| ceiling.saturating_sub(draw.drawn));
            let mut wire = serde_json::to_value(&value).expect("fixed budget serializes");
            // Brief 20 (E3): the top-level `reserved` key is a constant 0 on
            // this route — `budget()` runs with `exclude_engagement_id: None`
            // and the core assigns `reserved` only inside that arm
            // (allocation.rs:155-159). The meaningful commitment figures are
            // `pool.committed` and `draw.committed`; a constant 0 beside
            // them is noise, so it never leaves the server. The client
            // validator's key list drops it in the same commit.
            if let Some(object) = wire.as_object_mut() {
                object.remove("reserved");
            }
            wire["draw"] = serde_json::json!({
                "committed": draw.reserved,
                "measured": draw.spent,
                "consumed": draw.consumed,
                "drawn": draw.drawn,
                "ceilingTokens": draw.ceiling_tokens,
                "period": draw.period,
                "binding": if binding.is_empty() { serde_json::Value::Null } else { serde_json::json!(binding) },
                "remainingBeforeCeiling": remaining_before_ceiling,
            });
            bounded(res, &wire);
        }
        Err(error) => failure(res, error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Publication {
    expected_revision: String,
    published: bool,
}
#[handler]
async fn publication(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let deadline = Instant::now() + Duration::from_secs(2);
    let prepared = async {
        let id = resource_id(req)?;
        query(req, &[], 0)?;
        if req.headers().get_all("content-type").iter().count() != 1
            || req
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(';').next())
                .map(str::trim)
                != Some("application/json")
        {
            return Err(Error::Invalid);
        }
        let input: Publication =
            serde_json::from_slice(&body(req, 256).await?).map_err(|_| Error::Invalid)?;
        let session = depot
            .get_typed::<Session>()
            .map_err(|_| Error::Unauthorized)?;
        console(depot)?.0.authority.publication(
            session,
            id,
            input.expected_revision,
            input.published,
            deadline,
        )
    }
    .await;
    let command = match prepared {
        Ok(command) => command,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.publish_resource(command).await;
    if result.is_ok() && recheck(depot).is_err() {
        failure(res, hagency_store::Error::OutcomeUnknown);
        return;
    }
    match result {
        Ok(value) => bounded(res, &value),
        Err(error) => failure(res, error),
    }
}
