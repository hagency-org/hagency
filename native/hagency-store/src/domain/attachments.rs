//! Metadata admission and private host tickets share canonical writer authority.
use super::{DomainRepository, execution, matrix_routes, serialize, verified_ingress};
use crate::Error;
use hagency_core::{
    attachments::{AttachmentMetadata, MatrixAttachmentObservation},
    canonical,
    ingress::{MatrixIngressReceipt, MatrixIngressScope},
    messages::Message,
    replies::{ReplyRoute, matrix_event},
    tasks::RunnerCapability,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::json;

const MAX_ATTACHMENTS: u64 = 4096;
const MAX_PROJECTIONS: u64 = 16_384;
const MAX_SESSION_PROJECTIONS: u64 = 2048;

/// Host-only result, not a capability on its own. A download coordinator must
/// revalidate it with the current runner capability after all asynchronous IO.
#[derive(Clone)]
pub struct AttachmentTicket {
    capability_digest: String,
    route_digest: String,
    source_scope: MatrixIngressScope,
    source_sequence: u64,
    projection_sequence: u64,
    event_id: String,
    sdk_identity: String,
    manifest_id: String,
    content_digest: String,
    metadata: AttachmentMetadata,
}
impl AttachmentTicket {
    pub(crate) fn queue_value(&self) -> serde_json::Value {
        json!([
            self.capability_digest,
            self.route_digest,
            self.source_scope,
            self.source_sequence,
            self.projection_sequence,
            self.event_id,
            self.sdk_identity,
            self.manifest_id,
            self.content_digest,
            self.metadata
        ])
    }
    pub fn source_scope(&self) -> &MatrixIngressScope {
        &self.source_scope
    }
    pub fn source_sequence(&self) -> u64 {
        self.source_sequence
    }
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
    pub fn sdk_identity(&self) -> &str {
        &self.sdk_identity
    }
    pub fn manifest_id(&self) -> &str {
        &self.manifest_id
    }
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }
    pub fn metadata(&self) -> &AttachmentMetadata {
        &self.metadata
    }
}
fn attachment_digest(input: &MatrixAttachmentObservation) -> Result<String, Error> {
    Ok(canonical::digest(&json!([
        input.metadata,
        input.sdk_identity,
        input.manifest_id,
        input.content_digest
    ]))?)
}
pub(super) fn record(
    tx: &Transaction<'_>,
    route: &ReplyRoute,
    message: &Message,
    input: Option<&MatrixAttachmentObservation>,
    had_receipt: bool,
) -> Result<(), Error> {
    let old: Option<String> = tx
        .query_row(
            "SELECT digest FROM matrix_attachments WHERE engagement_id=?1 AND source_key=?2",
            params![route.engagement_id, message.source_key],
            |r| r.get(0),
        )
        .optional()?;
    let Some(input) = input else {
        return if old.is_some() {
            Err(Error::Conflict)
        } else {
            Ok(())
        };
    };
    input.validate()?;
    let digest = attachment_digest(input)?;
    if let Some(old) = old {
        return if old == digest {
            Ok(())
        } else {
            Err(Error::Conflict)
        };
    }
    // Neither a legacy file observation nor a previously admitted text event can
    // be retroactively annotated with a new private manifest.
    if had_receipt {
        return Err(Error::Conflict);
    }
    let metadata = serialize(&input.metadata)?;
    let changed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM matrix_attachments WHERE source_key=?1 AND (content_digest<>?2 OR metadata<>?3))",
        params![message.source_key,input.content_digest,metadata],|r|r.get(0),
    )?;
    if changed {
        return Err(Error::Conflict);
    }
    let count: u64 = tx.query_row("SELECT COUNT(*) FROM matrix_attachments", [], |r| r.get(0))?;
    if count >= MAX_ATTACHMENTS {
        return Err(Error::Capacity);
    }
    tx.execute("INSERT INTO matrix_attachments(engagement_id,source_key,message_sequence,source_session_id,digest,content_digest,metadata,sdk_identity,manifest_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![route.engagement_id,message.source_key,message.sequence,route.session_id,digest,input.content_digest,metadata,input.sdk_identity,input.manifest_id])?;
    Ok(())
}
pub(super) fn project_one(
    tx: &Transaction<'_>,
    route: &ReplyRoute,
    message: &Message,
) -> Result<(), Error> {
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM matrix_attachments WHERE engagement_id=?1 AND source_key=?2 AND message_sequence=?3)",params![route.engagement_id,message.source_key,message.sequence],|r|r.get(0))?;
    if !exists {
        return Ok(());
    }
    verified_ingress::provenance(tx, route, message)?;
    let exact: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM session_inputs WHERE session_id=?1 AND message_sequence=?2 AND config=?3)",params![route.session_id,message.sequence,serialize(message)?],|r|r.get(0))?;
    if !exact {
        return Err(Error::RunnerAuthority);
    }
    let old: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM session_attachment_visibility WHERE session_id=?1 AND message_sequence=?2 AND engagement_id=?3 AND source_key=?4)",params![route.session_id,message.sequence,route.engagement_id,message.source_key],|r|r.get(0))?;
    if old {
        return Ok(());
    }
    let (total, local): (u64,u64)=tx.query_row("SELECT (SELECT COUNT(*) FROM session_attachment_visibility),(SELECT COUNT(*) FROM session_attachment_visibility WHERE session_id=?1)",[&route.session_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if total >= MAX_PROJECTIONS || local >= MAX_SESSION_PROJECTIONS {
        return Err(Error::Capacity);
    }
    tx.execute("INSERT INTO session_attachment_visibility(session_id,engagement_id,source_key,message_sequence) VALUES(?1,?2,?3,?4)",params![route.session_id,route.engagement_id,message.source_key,message.sequence])?;
    Ok(())
}
pub(super) fn project_task_inputs(
    tx: &Transaction<'_>,
    task: &str,
    session: &str,
) -> Result<(), Error> {
    let native: bool = tx.query_row(
        "SELECT matrix_generation>0 FROM runner_sessions WHERE id=?1",
        [session],
        |r| r.get(0),
    )?;
    if !native {
        return Ok(());
    }
    let route = matrix_routes::route(tx, session)?;
    // Only the already-verified task input copies may acquire session visibility.
    let rows:Vec<String>=tx.prepare("SELECT ti.config FROM task_inputs ti JOIN matrix_attachments a ON a.message_sequence=ti.message_sequence AND a.engagement_id=?2 WHERE ti.task_id=?1 ORDER BY ti.message_sequence")?.query_map(params![task,route.engagement_id],|r|r.get(0))?.collect::<Result<_,_>>()?;
    for encoded in rows {
        project_one(tx, &route, &serde_json::from_str(&encoded)?)?;
    }
    Ok(())
}
pub(super) fn freeze(tx: &Transaction<'_>, dispatch: &str, session: &str) -> Result<(), Error> {
    let matrix: bool = tx.query_row(
        "SELECT matrix_generation>0 FROM runner_sessions WHERE id=?1",
        [session],
        |r| r.get(0),
    )?;
    if !matrix {
        return Ok(());
    }
    matrix_routes::check(tx, session)?;
    // Dispatches without an actual selected inbox have no file-read authority.
    tx.execute("INSERT INTO dispatch_attachment_windows(dispatch_id,session_id,source_cutoff,projection_cutoff) VALUES(?1,?2,0,0)",params![dispatch,session])?;
    Ok(())
}
/// Called only in the first inbox dispatch INSERT transaction, never on replay.
/// Later queued files must not expand the selected trigger's source boundary.
pub(super) fn freeze_inputs(
    tx: &Transaction<'_>,
    dispatch: &str,
    session: &str,
    source_cutoff: u64,
) -> Result<(), Error> {
    tx.execute("UPDATE dispatch_attachment_windows SET source_cutoff=?3,projection_cutoff=COALESCE((SELECT MAX(projection_sequence) FROM session_attachment_visibility WHERE session_id=?2),0) WHERE dispatch_id=?1 AND session_id=?2 AND source_cutoff=0 AND projection_cutoff=0",params![dispatch,session,source_cutoff])?;
    Ok(())
}
pub(super) fn ticket(
    db: &Connection,
    cap: &RunnerCapability,
    event_id: &str,
    now: u64,
) -> Result<AttachmentTicket, Error> {
    matrix_event(event_id)?;
    let dispatch = execution::authorize(db, cap, now, &["started"])?;
    if dispatch.report_task.is_some() {
        return Err(Error::RunnerAuthority);
    }
    let route = matrix_routes::route(db, &dispatch.session_id)?;
    let row:Option<(u64,u64,String,String,String,String,String)>=db.query_row("SELECT v.message_sequence,v.projection_sequence,a.source_session_id,a.metadata,a.sdk_identity,a.manifest_id,a.content_digest FROM dispatch_attachment_windows w JOIN session_attachment_visibility v ON v.session_id=w.session_id AND v.message_sequence<=w.source_cutoff AND v.projection_sequence<=w.projection_cutoff JOIN matrix_attachments a ON a.engagement_id=v.engagement_id AND a.source_key=v.source_key AND a.message_sequence=v.message_sequence JOIN session_inputs i ON i.session_id=v.session_id AND i.message_sequence=v.message_sequence WHERE w.dispatch_id=?1 AND w.session_id=?2 AND a.engagement_id=?3 AND json_extract(i.config,'$.event_id')=?4",params![cap.dispatch_id,route.session_id,route.engagement_id,event_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    let Some((
        sequence,
        projection,
        source_session,
        metadata,
        sdk_identity,
        manifest_id,
        content_digest,
    )) = row
    else {
        return Err(Error::RunnerAuthority);
    };
    let message = verified_ingress::input_message(db, &route.session_id, sequence)?;
    verified_ingress::provenance(db, &route, &message)?;
    let source_route = matrix_routes::route(db, &source_session)?;
    let since: u64 = db.query_row(
        "SELECT ingress_since FROM matrix_session_routes WHERE session_id=?1",
        [&route.session_id],
        |r| r.get(0),
    )?;
    if message.origin_ts < since {
        return Err(Error::RunnerAuthority);
    }
    Ok(AttachmentTicket {
        capability_digest: canonical::digest(&json!([
            cap.dispatch_id,
            cap.runner_id,
            cap.fence,
            cap.secret
        ]))?,
        route_digest: canonical::digest(&json!(route))?,
        source_scope: MatrixIngressScope::from(&source_route),
        source_sequence: sequence,
        projection_sequence: projection,
        event_id: event_id.into(),
        metadata: serde_json::from_str(&metadata)?,
        sdk_identity,
        manifest_id,
        content_digest,
    })
}
impl DomainRepository {
    pub fn admit_matrix_attachment(
        &mut self,
        input: &MatrixAttachmentObservation,
        now: u64,
    ) -> Result<MatrixIngressReceipt, Error> {
        input.validate()?;
        self.admit_matrix_input(&input.event, Some(input), now)
    }
    pub fn matrix_attachment_receipt(
        &self,
        input: &MatrixAttachmentObservation,
    ) -> Result<Option<MatrixIngressReceipt>, Error> {
        input.validate()?;
        let Some(receipt) = self.matrix_ingress_receipt(&input.event)? else {
            return Ok(None);
        };
        let old:Option<String>=self.db.query_row("SELECT a.digest FROM matrix_attachments a JOIN runner_sessions s ON s.engagement_id=a.engagement_id WHERE s.id=?1 AND a.message_sequence=?2 AND a.source_session_id=s.id",params![receipt.session_id,receipt.sequence],|r|r.get(0)).optional()?;
        if old.as_deref() != Some(&attachment_digest(input)?) {
            return Err(Error::Conflict);
        }
        Ok(Some(receipt))
    }
    pub fn authorize_attachment(
        &self,
        cap: &RunnerCapability,
        event_id: &str,
        now: u64,
    ) -> Result<AttachmentTicket, Error> {
        ticket(&self.db, cap, event_id, now)
    }
    pub fn revalidate_attachment(
        &self,
        cap: &RunnerCapability,
        prior: &AttachmentTicket,
        now: u64,
    ) -> Result<(), Error> {
        let current = ticket(&self.db, cap, &prior.event_id, now)?;
        if current.capability_digest != prior.capability_digest
            || current.route_digest != prior.route_digest
            || current.source_sequence != prior.source_sequence
            || current.projection_sequence != prior.projection_sequence
            || current.sdk_identity != prior.sdk_identity
            || current.manifest_id != prior.manifest_id
            || current.content_digest != prior.content_digest
            || current.metadata != prior.metadata
            || serialize(&current.source_scope)? != serialize(&prior.source_scope)?
        {
            return Err(Error::RunnerAuthority);
        }
        Ok(())
    }
}

/// Every returned candidate passes the exact existing ticket validator.
pub(super) fn visible(
    db: &Connection,
    cap: &RunnerCapability,
    after: u64,
    limit: usize,
    now: u64,
) -> Result<hagency_core::attachments::AttachmentPage, Error> {
    use hagency_core::attachments::{AttachmentPage, AttachmentSummary};
    hagency_core::tasks::clock(after)?;
    if !(1..=16).contains(&limit) {
        return Err(hagency_core::InvalidInput("attachment page must be 1..16").into());
    }
    let dispatch = execution::authorize(db, cap, now, &["started"])?;
    if dispatch.report_task.is_some() {
        return Err(Error::RunnerAuthority);
    }
    matrix_routes::route(db, &dispatch.session_id)?;
    let ids: Vec<String> = db.prepare("SELECT json_extract(i.config,'$.event_id') FROM dispatch_attachment_windows w JOIN session_attachment_visibility v ON v.session_id=w.session_id AND v.message_sequence<=w.source_cutoff AND v.projection_sequence<=w.projection_cutoff JOIN session_inputs i ON i.session_id=v.session_id AND i.message_sequence=v.message_sequence WHERE w.dispatch_id=?1 AND v.message_sequence>?2 AND json_extract(i.config,'$.origin_ts')>=(SELECT ingress_since FROM matrix_session_routes WHERE session_id=w.session_id) ORDER BY v.message_sequence LIMIT ?3")?
        .query_map(params![cap.dispatch_id, after, limit+1], |r| r.get(0))?.collect::<Result<_,_>>()?;
    let mut items = Vec::new();
    for id in ids {
        let t = ticket(db, cap, &id, now)?;
        items.push(AttachmentSummary {
            event_id: id,
            sequence: t.source_sequence,
            metadata: t.metadata,
        });
    }
    let next = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|v| v.sequence)
    } else {
        None
    };
    let page = AttachmentPage { items, next };
    if serde_json::to_vec(&page)?.len() > 16 * 1024 {
        return Err(Error::Capacity);
    }
    Ok(page)
}
impl DomainRepository {
    pub fn visible_attachments(
        &self,
        cap: &RunnerCapability,
        after: u64,
        limit: usize,
        now: u64,
    ) -> Result<hagency_core::attachments::AttachmentPage, Error> {
        visible(&self.db, cap, after, limit, now)
    }
}
impl DomainRepository {
    pub(crate) fn visible_attachments_clock(
        &mut self,
        cap: &RunnerCapability,
        after: u64,
        limit: usize,
        time: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<hagency_core::attachments::AttachmentPage, Error> {
        self.upload_transaction(time, |tx, now| visible(tx, cap, after, limit, now))
    }
}
