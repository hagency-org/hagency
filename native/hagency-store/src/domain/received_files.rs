//! Local cache correlation and one original write intent, never SDK/file proof.
use super::{DomainRepository, attachments, file_delivery, owned_dispatch};
use crate::{AttachmentTicket, Error};
use hagency_core::{
    attachments::AttachmentMetadata, canonical, received_files::*, tasks::RunnerCapability,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct ReceiveIdentity {
    id: String,
    binding_digest: String,
}
impl ReceiveIdentity {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn queue_value(&self) -> Value {
        json!([self.id, self.binding_digest])
    }
}
#[derive(Clone)]
pub struct ReceiveReservation {
    identity: ReceiveIdentity,
    binding: Binding,
}
impl ReceiveReservation {
    pub fn identity(&self) -> &ReceiveIdentity {
        &self.identity
    }
    pub fn metadata(&self) -> &AttachmentMetadata {
        &self.binding.metadata
    }
    pub fn limit(&self) -> usize {
        self.binding.limit
    }
    pub fn scope_fingerprint(&self) -> &str {
        &self.binding.scope
    }
    pub fn matches_ticket(&self, ticket: &AttachmentTicket) -> bool {
        self.binding.ticket == ticket.queue_value()
    }
    pub(crate) fn queue_value(&self) -> Value {
        json!([self.identity.queue_value(), self.binding])
    }
}
pub struct ReceiveAdmission {
    pub identity: ReceiveIdentity,
    pub observation: ReceivedFileObservation,
    /// Only first reservation returns the original association for receiving.
    pub reservation: Option<ReceiveReservation>,
}
/// Only the first committed transition returns this value. No Clone or serde.
pub struct ReceiveWrite {
    reservation: ReceiveReservation,
    facts: ReceivedFileFacts,
}
impl ReceiveWrite {
    /// Read-only association with the complete originally reserved capability.
    /// A match grants no current authority; the retained writer checks that too.
    pub fn matches_capability(&self, cap: &RunnerCapability) -> bool {
        cap_digest(cap).is_ok_and(|digest| digest == self.reservation.binding.capability_digest)
    }
    pub fn identity(&self) -> &ReceiveIdentity {
        self.reservation.identity()
    }
    pub fn scope_fingerprint(&self) -> &str {
        self.reservation.scope_fingerprint()
    }
    pub fn facts(&self) -> &ReceivedFileFacts {
        &self.facts
    }
    pub fn metadata(&self) -> &AttachmentMetadata {
        self.reservation.metadata()
    }
    pub fn limit(&self) -> usize {
        self.reservation.limit()
    }
    pub fn matches_ticket(&self, ticket: &AttachmentTicket) -> bool {
        self.reservation.matches_ticket(ticket)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Binding {
    capability_digest: String,
    event_id: String,
    ticket: Value,
    scope: String,
    workspace: String,
    limit: usize,
    metadata: AttachmentMetadata,
}
fn cap_digest(cap: &RunnerCapability) -> Result<String, Error> {
    file_delivery::cap_input(cap)?;
    Ok(canonical::digest(&json!(cap))?)
}
fn bind(
    db: &Connection,
    cap: &RunnerCapability,
    event: &str,
    limit: usize,
    now: u64,
) -> Result<Binding, Error> {
    receive_limit(limit)?;
    let capability_digest = cap_digest(cap)?;
    let ticket = attachments::ticket(db, cap, event, now)?;
    let scope = owned_dispatch::scope(db, cap, now, &["started"])?;
    if scope.input().resources.len() != 1 || !scope.input().resources[0].exclusive {
        return Err(Error::RunnerAuthority);
    }
    Ok(Binding {
        capability_digest,
        event_id: event.into(),
        ticket: ticket.queue_value(),
        scope: scope.fingerprint().into(),
        workspace: scope.input().resources[0].id.clone(),
        limit,
        metadata: ticket.metadata().clone(),
    })
}
fn identity(binding: &Binding) -> Result<ReceiveIdentity, Error> {
    Ok(ReceiveIdentity {
        id: format!(
            "receive_{}",
            &canonical::digest(&json!([
                "received_file_v1",
                binding.capability_digest,
                binding.event_id
            ]))?[..32]
        ),
        binding_digest: canonical::payload_digest(&json!(binding))?,
    })
}
fn id_input(id: &str) -> Result<(), Error> {
    if id.len() != 40
        || !id.starts_with("receive_")
        || !id[8..]
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return Err(hagency_core::InvalidInput("invalid received file selector").into());
    }
    Ok(())
}
struct Row {
    identity: ReceiveIdentity,
    binding: Binding,
    facts: Option<ReceivedFileFacts>,
    state: ReceivedFileState,
    failure: Option<ReceiveFailure>,
}
fn read(db: &Connection, id: &str) -> Result<Row, Error> {
    id_input(id)?;
    struct StoredRow {
        encoded: String,
        hash: String,
        cap: String,
        workspace: String,
        limit: usize,
        facts: Option<String>,
        state: String,
        failure: Option<String>,
        event: String,
    }
    let raw = db.query_row(
        "SELECT binding,binding_digest,capability_digest,workspace_id,byte_limit,facts,state,failure,event_id FROM received_files WHERE id=?1", [id],
        |r| Ok(StoredRow {
            encoded: r.get(0)?, hash: r.get(1)?, cap: r.get(2)?, workspace: r.get(3)?,
            limit: r.get(4)?, facts: r.get(5)?, state: r.get(6)?, failure: r.get(7)?, event: r.get(8)?,
        }),
    ).optional()?;
    let StoredRow {
        encoded,
        hash,
        cap,
        workspace,
        limit,
        facts,
        state,
        failure,
        event,
    } = raw.ok_or(Error::NotFound)?;
    if encoded.len() > 16 * 1024 {
        return Err(Error::Schema);
    }
    let binding: Binding = serde_json::from_str(&encoded)?;
    receive_limit(binding.limit)?;
    binding.metadata.validate()?;
    hagency_core::replies::matrix_event(&binding.event_id)?;
    hagency_core::uploads::digest(&binding.capability_digest)?;
    hagency_core::uploads::digest(&binding.scope)?;
    hagency_core::project::identifier(&binding.workspace, 128)?;
    let original = identity(&binding)?;
    if original.id != id
        || original.binding_digest != hash
        || binding.capability_digest != cap
        || binding.workspace != workspace
        || binding.limit != limit
        || binding.event_id != event
    {
        return Err(Error::Schema);
    }
    let row = Row {
        identity: original,
        binding,
        facts: facts.map(|v| serde_json::from_str(&v)).transpose()?,
        state: serde_json::from_value(json!(state))?,
        failure: failure
            .map(|v| serde_json::from_value(json!(v)))
            .transpose()?,
    };
    if let Some(facts) = &row.facts {
        facts.validate(row.binding.limit)?;
    }
    row.observation(false)?.validate()?;
    Ok(row)
}
impl Row {
    fn observation(&self, replayed: bool) -> Result<ReceivedFileObservation, Error> {
        let value = ReceivedFileObservation {
            id: self.identity.id.clone(),
            event_id: self.binding.event_id.clone(),
            metadata: self.binding.metadata.clone(),
            facts: self.facts.clone(),
            state: self.state,
            error_code: self.failure,
            replayed,
        };
        value.validate()?;
        if serde_json::to_vec(&value)?.len() > 4096 {
            return Err(Error::Capacity);
        }
        Ok(value)
    }
    fn original(&self, cap: &RunnerCapability, identity: &ReceiveIdentity) -> Result<(), Error> {
        if cap_digest(cap)? != self.binding.capability_digest
            || self.identity.queue_value() != identity.queue_value()
        {
            return Err(Error::RunnerAuthority);
        }
        Ok(())
    }
    fn current(&self, db: &Connection, cap: &RunnerCapability, now: u64) -> Result<(), Error> {
        if bind(db, cap, &self.binding.event_id, self.binding.limit, now)? != self.binding {
            return Err(Error::RunnerAuthority);
        }
        Ok(())
    }
}
pub(crate) fn reserve(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    event: &str,
    limit: usize,
    now: u64,
) -> Result<ReceiveAdmission, Error> {
    let binding = bind(tx, cap, event, limit, now)?;
    let original = identity(&binding)?;
    match read(tx, &original.id) {
        Ok(row) => {
            if row.binding != binding {
                return Err(Error::Conflict);
            }
            return Ok(ReceiveAdmission {
                identity: row.identity.clone(),
                observation: row.observation(true)?,
                reservation: None,
            });
        }
        Err(Error::NotFound) => (),
        Err(e) => return Err(e),
    }
    let (all,own,bytes):(usize,usize,u64)=tx.query_row("SELECT COUNT(*),COUNT(*) FILTER(WHERE workspace_id=?1),COALESCE(SUM(byte_limit),0) FROM received_files",[&binding.workspace],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    if all >= MAX_RECEIVED_FILES
        || own >= MAX_WORKSPACE_RECEIVED_FILES
        || bytes
            .checked_add(limit as u64)
            .is_none_or(|n| n > MAX_RECEIVED_RESERVED_BYTES)
    {
        return Err(Error::Capacity);
    }
    let encoded = serde_json::to_string(&binding)?;
    if encoded.len() > 16 * 1024 {
        return Err(Error::Capacity);
    }
    tx.execute("INSERT INTO received_files(id,capability_digest,event_id,workspace_id,binding,binding_digest,byte_limit,state) VALUES(?1,?2,?3,?4,?5,?6,?7,'reserved')",params![original.id,binding.capability_digest,event,binding.workspace,encoded,original.binding_digest,limit])?;
    let row = read(tx, &original.id)?;
    Ok(ReceiveAdmission {
        identity: original.clone(),
        observation: row.observation(false)?,
        reservation: Some(ReceiveReservation {
            identity: original,
            binding,
        }),
    })
}
pub(crate) fn begin(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    reservation: &ReceiveReservation,
    facts: &ReceivedFileFacts,
    now: u64,
) -> Result<ReceiveWrite, Error> {
    facts.validate(reservation.limit())?;
    let row = read(tx, reservation.identity.id())?;
    row.original(cap, &reservation.identity)?;
    if row.binding != reservation.binding {
        return Err(Error::Conflict);
    }
    if row.facts.as_ref().is_some_and(|old| old != facts) {
        return Err(Error::Conflict);
    }
    row.current(tx, cap, now)?;
    if row.state != ReceivedFileState::Reserved {
        return Err(Error::OutcomeUnknown);
    }
    tx.execute(
        "UPDATE received_files SET state='write_possible',facts=?2 WHERE id=?1",
        params![row.identity.id, serde_json::to_string(facts)?],
    )?;
    Ok(ReceiveWrite {
        reservation: reservation.clone(),
        facts: facts.clone(),
    })
}
pub(crate) fn ready(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    original: &ReceiveIdentity,
    facts: &ReceivedFileFacts,
    now: u64,
) -> Result<ReceivedFileObservation, Error> {
    let row = read(tx, original.id())?;
    row.original(cap, original)?;
    facts.validate(row.binding.limit)?;
    if row.facts.as_ref() != Some(facts) {
        return Err(Error::Conflict);
    }
    row.current(tx, cap, now)?;
    if row.state == ReceivedFileState::Ready {
        return row.observation(true);
    }
    if row.state != ReceivedFileState::WritePossible {
        return Err(Error::State);
    }
    tx.execute(
        "UPDATE received_files SET state='ready' WHERE id=?1",
        [original.id()],
    )?;
    read(tx, original.id())?.observation(false)
}
pub(crate) fn negative(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    original: &ReceiveIdentity,
    failure: ReceiveFailure,
) -> Result<ReceivedFileObservation, Error> {
    let row = read(tx, original.id())?;
    row.original(cap, original)?;
    if matches!(
        row.state,
        ReceivedFileState::Ready | ReceivedFileState::Failed | ReceivedFileState::OutcomeUnknown
    ) {
        return row.observation(true);
    }
    let state = if row.state == ReceivedFileState::WritePossible
        || failure == ReceiveFailure::OutcomeUnknown
    {
        "outcome_unknown"
    } else {
        "failed"
    };
    let failure = serde_json::to_value(failure)?;
    tx.execute(
        "UPDATE received_files SET state=?2,failure=?3 WHERE id=?1",
        params![original.id(), state, failure.as_str().ok_or(Error::Schema)?],
    )?;
    read(tx, original.id())?.observation(false)
}
pub(crate) fn inspect(
    db: &Connection,
    cap: &RunnerCapability,
    id: &str,
) -> Result<ReceivedFileObservation, Error> {
    let row = read(db, id)?;
    row.original(cap, &row.identity)?;
    row.observation(true)
}
impl DomainRepository {
    pub fn reserve_received_file(
        &mut self,
        cap: &RunnerCapability,
        event: &str,
        limit: usize,
        now: u64,
    ) -> Result<ReceiveAdmission, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| reserve(tx, cap, event, limit, n))
    }
    pub fn start_received_file_write(
        &mut self,
        cap: &RunnerCapability,
        reservation: &ReceiveReservation,
        facts: &ReceivedFileFacts,
        now: u64,
    ) -> Result<ReceiveWrite, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| begin(tx, cap, reservation, facts, n))
    }
    pub fn record_received_file_ready(
        &mut self,
        cap: &RunnerCapability,
        original: &ReceiveIdentity,
        facts: &ReceivedFileFacts,
        now: u64,
    ) -> Result<ReceivedFileObservation, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| ready(tx, cap, original, facts, n))
    }
    pub fn record_received_file_negative(
        &mut self,
        cap: &RunnerCapability,
        original: &ReceiveIdentity,
        failure: ReceiveFailure,
    ) -> Result<ReceivedFileObservation, Error> {
        self.upload_transaction(|| Ok(0), |tx, _| negative(tx, cap, original, failure))
    }
    pub fn inspect_received_file(
        &self,
        cap: &RunnerCapability,
        id: &str,
    ) -> Result<ReceivedFileObservation, Error> {
        inspect(&self.db, cap, id)
    }
}
