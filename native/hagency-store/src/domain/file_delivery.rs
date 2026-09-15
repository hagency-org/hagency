//! Original file metadata and separate current/historical room-event custody.
use super::{DomainRepository, execution, serialize, uploads};
use crate::{Error, UploadAdmission, UploadClaim, UploadIdentity, UploadPreparation};
use hagency_core::{canonical, file_delivery::*, replies::*, tasks::RunnerCapability, uploads::*};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct FileDeliveryIdentity {
    id: String,
    upload: UploadIdentity,
}
impl FileDeliveryIdentity {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn queue_value(&self) -> Value {
        json!([self.id, self.upload.queue_value()])
    }
}
pub struct FileDeliveryAdmission {
    pub identity: FileDeliveryIdentity,
    pub receipt: FileDeliveryReceipt,
    pub upload: UploadAdmission,
}
#[derive(Clone)]
pub struct FilePublicationClaim {
    identity: FileDeliveryIdentity,
    fence: u64,
    secret: String,
}
impl FilePublicationClaim {
    pub fn fence(&self) -> u64 {
        self.fence
    }
    pub(crate) fn queue_value(&self) -> Value {
        json!([self.identity.queue_value(), self.fence, self.secret])
    }
}
/// Only durable first begin returns a send. No Clone, raw constructor or serde.
pub struct FilePublicationSend {
    identity: FileDeliveryIdentity,
    locator: FilePublicationLocator,
    request: FileDeliveryRequest,
    captured: CapturedFile,
}
impl FilePublicationSend {
    /// Borrow the exact original identity for retained cancellation/uncertainty.
    pub fn identity(&self) -> &FileDeliveryIdentity {
        &self.identity
    }
    pub fn locator(&self) -> &FilePublicationLocator {
        &self.locator
    }
    pub fn metadata(&self) -> &FileDeliveryRequest {
        &self.request
    }
    pub fn captured(&self) -> &CapturedFile {
        &self.captured
    }
    pub fn matches_claim(&self, claim: &FilePublicationClaim) -> bool {
        self.identity.queue_value() == claim.identity.queue_value()
            && self.locator.fence == claim.fence
    }
    /// Exact historical upload association only, not current publication proof.
    pub fn matches_upload_claim(&self, upload: &UploadClaim) -> bool {
        upload.matches_identity(&self.identity.upload)
            && self.locator.upload_fence == upload.fence()
    }
}
/// Exact historical association only. No identity/claim/send conversion.
pub struct FileDeliverySettlement {
    identity: FileDeliveryIdentity,
    locator: FilePublicationLocator,
}
impl FileDeliverySettlement {
    pub fn id(&self) -> &str {
        &self.identity.id
    }
    pub(crate) fn queue_value(&self) -> Value {
        json!([self.identity.queue_value(), self.locator])
    }
}
struct Row {
    identity: FileDeliveryIdentity,
    request: FileDeliveryRequest,
    captured: Option<CapturedFile>,
    event: FileEventState,
    fence: u64,
    hash: Option<String>,
    until: Option<u64>,
    transaction: String,
    publication: Option<FilePublicationLocator>,
    cancelled: bool,
    failure: Option<FileDeliveryFailure>,
    acceptance: Option<String>,
    upload: uploads::UploadContext,
}
fn bounded(value: &impl serde::Serialize) -> Result<String, Error> {
    let text = serialize(value)?;
    if text.len() > MAX_FILE_RECORD_BYTES {
        return Err(Error::Capacity);
    }
    Ok(text)
}
pub(crate) fn cap_input(cap: &RunnerCapability) -> Result<(), Error> {
    hagency_core::project::identifier(&cap.dispatch_id, 128)?;
    hagency_core::project::identifier(&cap.runner_id, 128)?;
    digest(&cap.secret)?;
    generation(cap.fence)?;
    Ok(())
}
pub(crate) fn request_input(
    cap: &RunnerCapability,
    request: &FileDeliveryRequest,
) -> Result<(), Error> {
    cap_input(cap)?;
    request.validate()?;
    bounded(request)?;
    Ok(())
}
fn upload_request(request: &FileDeliveryRequest) -> Result<UploadRequest, Error> {
    request.validate()?;
    Ok(UploadRequest {
        call_id: request.call_id.clone(),
        request_digest: canonical::payload_digest(&json!(["file_request_v1", request]))?,
        metadata: hagency_core::attachments::AttachmentMetadata {
            filename: request.filename.clone(),
            mime_type: Some(FILE_MIME.into()),
            declared_size: None,
        },
    })
}
fn original(
    cap: &RunnerCapability,
    request: &FileDeliveryRequest,
) -> Result<FileDeliveryIdentity, Error> {
    request_input(cap, request)?;
    Ok(FileDeliveryIdentity {
        id: format!(
            "file_{}",
            &canonical::digest(&json!([
                "file_delivery_v1",
                cap.dispatch_id,
                request.call_id
            ]))?[..32]
        ),
        upload: uploads::original(cap, &upload_request(request)?)?,
    })
}
fn read(db: &Connection, id: &str) -> Result<Row, Error> {
    let raw: Option<String> = db.query_row("SELECT json_object('upload',upload_id,'request',json(request),'request_hash',request_hash,'captured',json(captured),'event',event_state,'fence',claim_fence,'hash',claim_hash,'until',claim_until,'transaction',transaction_id,'publication',json(publication),'cancelled',cancel_requested,'failure',failure,'acceptance',acceptance) FROM file_deliveries WHERE id=?1",[id],|r|r.get(0)).optional()?;
    let v: Value = serde_json::from_str(&raw.ok_or(Error::NotFound)?)?;
    let field =
        |key: &str| -> Result<String, Error> { Ok(v[key].as_str().ok_or(Error::Schema)?.into()) };
    let request: FileDeliveryRequest = serde_json::from_value(v["request"].clone())?;
    request.validate()?;
    if canonical::payload_digest(&json!(["file_request_v1", request]))? != field("request_hash")? {
        return Err(Error::Schema);
    }
    let identity = FileDeliveryIdentity {
        id: id.into(),
        upload: uploads::file_identity(db, &field("upload")?)?,
    };
    let upload = uploads::file_context(db, &identity.upload)?;
    let row = Row {
        identity,
        request,
        upload,
        captured: serde_json::from_value(v["captured"].clone())?,
        event: serde_json::from_value(v["event"].clone())?,
        fence: v["fence"].as_u64().ok_or(Error::Schema)?,
        hash: v["hash"].as_str().map(str::to_owned),
        until: v["until"].as_u64(),
        transaction: field("transaction")?,
        publication: serde_json::from_value(v["publication"].clone())?,
        cancelled: v["cancelled"] == 1,
        failure: serde_json::from_value(v["failure"].clone())?,
        acceptance: v["acceptance"].as_str().map(str::to_owned),
    };
    if let Some(captured) = &row.captured {
        captured.validate()?;
    }
    row.payload_bound()?;
    Ok(row)
}
impl Row {
    fn payload_bound(&self) -> Result<(), Error> {
        bounded(&json!([
            self.request,
            self.captured,
            self.publication,
            self.acceptance
        ]))?;
        Ok(())
    }
    fn receipt(&self, replayed: bool) -> Result<FileDeliveryReceipt, Error> {
        let unknown = self.upload.receipt.outcome_unknown
            || self.upload.receipt.stage == UploadStageState::Unknown
            || self.upload.receipt.upload == UploadState::WritePossible
            || self.event == FileEventState::WritePossible;
        let cancelled = self.cancelled || self.upload.receipt.cancel_requested;
        let status = if self.event == FileEventState::Delivered {
            FileDeliveryStatus::Delivered
        } else if unknown {
            FileDeliveryStatus::OutcomeUnknown
        } else if cancelled {
            FileDeliveryStatus::Failed
        } else {
            FileDeliveryStatus::Queued
        };
        let event_id = self
            .acceptance
            .as_ref()
            .map(|raw| -> Result<String, Error> {
                let v: Value = serde_json::from_str(raw)?;
                Ok(v["event_id"].as_str().ok_or(Error::Schema)?.into())
            })
            .transpose()?;
        Ok(FileDeliveryReceipt {
            id: self.identity.id.clone(),
            filename: self.request.filename.clone(),
            captured: self.captured.clone(),
            stage: self.upload.receipt.stage,
            upload: self.upload.receipt.upload,
            event: self.event,
            status,
            cancel_requested: cancelled,
            error_code: if cancelled {
                Some(self.failure.unwrap_or(FileDeliveryFailure::Cancelled))
            } else {
                None
            },
            event_id,
            replayed,
        })
    }
}
fn identity(db: &Connection, id: &FileDeliveryIdentity) -> Result<Row, Error> {
    let row = read(db, &id.id)?;
    if row.identity.upload.queue_value() != id.upload.queue_value() {
        return Err(Error::RunnerAuthority);
    }
    Ok(row)
}
fn current(db: &Connection, cap: &RunnerCapability, row: &Row, now: u64) -> Result<(), Error> {
    cap_input(cap)?;
    if row.cancelled {
        return Err(Error::RunnerAuthority);
    }
    uploads::file_current(db, cap, &row.identity.upload, now)
}
fn token(db: &Connection, claim: &FilePublicationClaim, now: u64) -> Result<Row, Error> {
    let row = identity(db, &claim.identity)?;
    if row.fence != claim.fence
        || row.until.is_none_or(|until| until <= now)
        || !execution::matches_secret(row.hash.as_deref().unwrap_or(""), &claim.secret)?
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(row)
}
fn locator(row: &Row) -> Result<FilePublicationLocator, Error> {
    if row.upload.receipt.upload != UploadState::Accepted
        || row.upload.receipt.stage != UploadStageState::Staged
    {
        return Err(Error::State);
    }
    let captured = row.captured.as_ref().ok_or(Error::State)?;
    let stage = row.upload.stage.clone().ok_or(Error::Schema)?;
    if captured.size != stage.len {
        return Err(Error::Schema);
    }
    let accepted: Value =
        serde_json::from_str(row.upload.acceptance.as_deref().ok_or(Error::Schema)?)?;
    let observed = UploadAcceptance {
        receipt_id: accepted["receipt_id"].as_str().ok_or(Error::Schema)?.into(),
        receipt_digest: accepted["receipt_digest"]
            .as_str()
            .ok_or(Error::Schema)?
            .into(),
    };
    observed.validate()?;
    let content_digest = canonical::payload_digest(&json!([
        "file_publication_v1",
        row.identity.queue_value(),
        row.request,
        captured,
        FILE_MIME,
        row.upload.scope,
        row.upload.route,
        row.upload.fence,
        stage,
        observed
    ]))?;
    Ok(FilePublicationLocator {
        delivery_id: row.identity.id.clone(),
        fence: row.fence,
        transaction_id: row.transaction.clone(),
        content_digest,
        upload_id: row.identity.upload.id().into(),
        upload_fence: row.upload.fence,
        stage,
        route: row.upload.route.clone(),
        upload_receipt_id: observed.receipt_id,
        upload_receipt_digest: observed.receipt_digest,
    })
}
pub(crate) fn lookup_input(input: &FilePublicationLocator) -> Result<(), Error> {
    id_input(&input.delivery_id)?;
    generation(input.fence)?;
    hagency_core::project::identifier(&input.transaction_id, 128)?;
    digest(&input.content_digest)?;
    hagency_core::project::identifier(&input.upload_receipt_id, 128)?;
    digest(&input.upload_receipt_digest)?;
    uploads::settlement_lookup(
        &input.upload_id,
        input.upload_fence,
        &input.stage,
        &input.route,
    )?;
    bounded(input)?;
    Ok(())
}
pub(crate) fn id_input(id: &str) -> Result<(), Error> {
    if id.len() != 37
        || !id.starts_with("file_")
        || !id[5..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(hagency_core::InvalidInput("invalid file delivery selector").into());
    }
    Ok(())
}
pub(crate) fn reserve(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    request: &FileDeliveryRequest,
    now: u64,
) -> Result<FileDeliveryAdmission, Error> {
    let id = original(cap, request)?;
    match read(tx, &id.id) {
        Ok(row) => {
            if row.request != *request {
                return Err(Error::Conflict);
            }
            identity(tx, &id)?;
            let upload = uploads::reserve(tx, cap, &upload_request(request)?, now)?;
            if upload.preparation.is_some() {
                return Err(Error::Schema);
            }
            return Ok(FileDeliveryAdmission {
                identity: id,
                receipt: row.receipt(true)?,
                upload,
            });
        }
        Err(Error::NotFound) => (),
        Err(error) => return Err(error),
    }
    let (all, own): (usize, usize) = tx.query_row(
        "SELECT COUNT(*),COUNT(*) FILTER(WHERE dispatch_id=?1) FROM file_deliveries",
        [&cap.dispatch_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if all >= MAX_FILE_DELIVERIES || own >= MAX_DISPATCH_FILE_DELIVERIES {
        return Err(Error::Capacity);
    }
    let upload = uploads::reserve(tx, cap, &upload_request(request)?, now)?;
    if upload.preparation.is_none() {
        return Err(Error::Conflict);
    }
    let context = uploads::file_context(tx, &upload.identity)?;
    bounded(&json!([request, context.route]))?;
    let transaction = format!(
        "file_event_{}",
        &canonical::digest(&json!(["file_event_v1", id.queue_value()]))?[..32]
    );
    tx.execute("INSERT INTO file_deliveries(id,upload_id,dispatch_id,call_id,request,request_hash,event_state,transaction_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7,?8,?8)",params![id.id,id.upload.id(),cap.dispatch_id,request.call_id,bounded(request)?,canonical::payload_digest(&json!(["file_request_v1",request]))?,transaction,now])?;
    Ok(FileDeliveryAdmission {
        receipt: read(tx, &id.id)?.receipt(false)?,
        identity: id,
        upload,
    })
}
pub(crate) fn bind(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    id: &FileDeliveryIdentity,
    preparation: &UploadPreparation,
    captured: &CapturedFile,
    stage: &StageCommitment,
    now: u64,
) -> Result<FileDeliveryReceipt, Error> {
    captured.validate()?;
    stage.validate()?;
    if captured.size != stage.len {
        return Err(hagency_core::InvalidInput("captured and staged byte lengths differ").into());
    }
    let row = identity(tx, id)?;
    current(tx, cap, &row, now)?;
    if !preparation.matches_identity(&id.upload) {
        return Err(Error::RunnerAuthority);
    }
    if row.captured.is_none()
        && (row.upload.receipt.upload != UploadState::Pending
            || !matches!(
                row.upload.receipt.stage,
                UploadStageState::Unbound | UploadStageState::Bound
            ))
    {
        return Err(Error::State);
    }
    if row.captured.as_ref().is_some_and(|old| old != captured) {
        return Err(Error::Conflict);
    }
    bounded(&json!([
        row.request,
        captured,
        row.publication,
        row.acceptance
    ]))?;
    let replayed = row.captured.is_some();
    uploads::bind(tx, cap, preparation, stage, now)?;
    tx.execute(
        "UPDATE file_deliveries SET captured=?2,updated_at=?3 WHERE id=?1",
        params![id.id, serialize(captured)?, now],
    )?;
    read(tx, &id.id)?.receipt(replayed)
}
pub(crate) fn claim(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    id: &FileDeliveryIdentity,
    now: u64,
    lease: u64,
) -> Result<Option<FilePublicationClaim>, Error> {
    if !(1..=60000).contains(&lease) {
        return Err(hagency_core::InvalidInput("invalid publication lease").into());
    }
    let row = identity(tx, id)?;
    current(tx, cap, &row, now)?;
    if row.upload.receipt.upload != UploadState::Accepted
        || row.captured.is_none()
        || matches!(
            row.event,
            FileEventState::WritePossible | FileEventState::Delivered
        )
        || row.event == FileEventState::Claimed && row.until.is_some_and(|until| until > now)
    {
        return Ok(None);
    }
    let fence = row
        .fence
        .checked_add(1)
        .filter(|v| *v <= hagency_core::JSON_SAFE_MAX)
        .ok_or(Error::Capacity)?;
    let until = now
        .checked_add(lease)
        .filter(|v| *v <= hagency_core::JSON_SAFE_MAX)
        .ok_or(Error::Capacity)?;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    tx.execute("UPDATE file_deliveries SET event_state='claimed',claim_fence=?2,claim_hash=?3,claim_until=?4,updated_at=?5 WHERE id=?1",params![id.id,fence,canonical::digest(&json!(secret))?,until,now])?;
    Ok(Some(FilePublicationClaim {
        identity: id.clone(),
        fence,
        secret,
    }))
}
pub(crate) fn begin(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    claim: &FilePublicationClaim,
    now: u64,
) -> Result<FilePublicationSend, Error> {
    let row = token(tx, claim, now)?;
    current(tx, cap, &row, now)?;
    if row.event != FileEventState::Claimed {
        return Err(Error::State);
    }
    let publication = locator(&row)?;
    bounded(&json!([
        row.request,
        row.captured,
        publication,
        row.acceptance
    ]))?;
    tx.execute("UPDATE file_deliveries SET event_state='write_possible',publication=?2,updated_at=?3 WHERE id=?1",params![row.identity.id,bounded(&publication)?,now])?;
    Ok(FilePublicationSend {
        identity: row.identity,
        locator: publication,
        request: row.request,
        captured: row.captured.ok_or(Error::Schema)?,
    })
}
pub(crate) fn validate(
    tx: &Transaction<'_>,
    cap: &RunnerCapability,
    claim: &FilePublicationClaim,
    now: u64,
) -> Result<(), Error> {
    let row = token(tx, claim, now)?;
    current(tx, cap, &row, now)?;
    if row.event != FileEventState::WritePossible
        || row.publication.as_ref() != Some(&locator(&row)?)
    {
        return Err(Error::State);
    }
    Ok(())
}
pub(crate) fn cancel(
    tx: &Transaction<'_>,
    id: &FileDeliveryIdentity,
    reason: FileDeliveryFailure,
    now: u64,
) -> Result<FileDeliveryReceipt, Error> {
    let row = identity(tx, id)?;
    uploads::cancel(tx, &id.upload, now)?;
    tx.execute("UPDATE file_deliveries SET cancel_requested=1,failure=COALESCE(failure,?2),claim_hash=NULL,claim_until=NULL,updated_at=?3 WHERE id=?1",params![id.id,serde_json::to_value(reason)?.as_str().ok_or(Error::Schema)?,now])?;
    read(tx, &id.id)?.receipt(row.cancelled)
}
pub(crate) fn uncertain(
    tx: &Transaction<'_>,
    id: &FileDeliveryIdentity,
    fence: u64,
    now: u64,
) -> Result<FileDeliveryReceipt, Error> {
    let row = identity(tx, id)?;
    if row.fence != fence || row.event != FileEventState::WritePossible {
        return Err(Error::State);
    }
    tx.execute(
        "UPDATE file_deliveries SET claim_hash=NULL,claim_until=NULL,updated_at=?2 WHERE id=?1",
        params![id.id, now],
    )?;
    read(tx, &id.id)?.receipt(true)
}
fn settlement_row(db: &Connection, settlement: &FileDeliverySettlement) -> Result<Row, Error> {
    let row = identity(db, &settlement.identity)?;
    if !matches!(
        row.event,
        FileEventState::WritePossible | FileEventState::Delivered
    ) || row.publication.as_ref() != Some(&settlement.locator)
        || locator(&row)? != settlement.locator
    {
        return Err(Error::RunnerAuthority);
    }
    Ok(row)
}
pub(crate) fn settle(
    tx: &Transaction<'_>,
    settlement: &FileDeliverySettlement,
    observed: &FileDeliveryAcceptance,
    now: u64,
) -> Result<FileDeliveryReceipt, Error> {
    observed.validate()?;
    let row = settlement_row(tx, settlement)?;
    if observed.transaction_id != settlement.locator.transaction_id
        || observed.content_digest != settlement.locator.content_digest
    {
        return Err(Error::RunnerAuthority);
    }
    let receipt = bounded(observed)?;
    if let Some(old) = &row.acceptance {
        if *old != receipt {
            return Err(Error::Conflict);
        }
        return row.receipt(true);
    }
    bounded(&json!([
        row.request,
        row.captured,
        row.publication,
        receipt
    ]))?;
    tx.execute("UPDATE file_deliveries SET event_state='delivered',acceptance=?2,claim_hash=NULL,claim_until=NULL,updated_at=?3 WHERE id=?1",params![row.identity.id,receipt,now])?;
    read(tx, &row.identity.id)?.receipt(false)
}
/// Completion guard for one dispatch. A delivery that is not delivered stays
/// unsettled while it has no recorded failure, and permanently while its event
/// or upload is a possible external write: the pipeline records a failure on
/// such rows without knowing whether the homeserver kept the write. Ordinary
/// runner completion can never promote either case to success.
pub(super) fn complete_guard(db: &Connection, dispatch_id: &str) -> Result<(), Error> {
    let unsettled: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM file_deliveries f JOIN file_uploads u ON u.id=f.upload_id WHERE f.dispatch_id=?1 AND f.event_state<>'delivered' AND (f.failure IS NULL OR f.event_state='write_possible' OR u.upload_state='write_possible' OR u.outcome_unknown=1))",
        [dispatch_id],
        |r| r.get(0),
    )?;
    if unsettled {
        return Err(Error::State);
    }
    Ok(())
}
impl DomainRepository {
    pub fn reserve_file_delivery(
        &mut self,
        cap: &RunnerCapability,
        request: &FileDeliveryRequest,
        now: u64,
    ) -> Result<FileDeliveryAdmission, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| reserve(tx, cap, request, n))
    }
    pub fn restore_file_delivery(
        &self,
        cap: &RunnerCapability,
        request: &FileDeliveryRequest,
    ) -> Result<Option<FileDeliveryIdentity>, Error> {
        let id = original(cap, request)?;
        match read(&self.db, &id.id) {
            Ok(row) => {
                if row.request != *request {
                    return Err(Error::Conflict);
                }
                identity(&self.db, &id)?;
                Ok(Some(id))
            }
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn inspect_file_delivery(
        &self,
        cap: &RunnerCapability,
        id: &str,
    ) -> Result<FileDeliveryReceipt, Error> {
        cap_input(cap)?;
        id_input(id)?;
        let row = read(&self.db, id).map_err(|e| {
            if matches!(e, Error::NotFound) {
                Error::RunnerAuthority
            } else {
                e
            }
        })?;
        let expected = original(cap, &row.request)?;
        if expected.id != id || expected.upload.queue_value() != row.identity.upload.queue_value() {
            return Err(Error::RunnerAuthority);
        }
        row.receipt(true)
    }
    pub fn bind_file_delivery_stage(
        &mut self,
        cap: &RunnerCapability,
        id: &FileDeliveryIdentity,
        preparation: &UploadPreparation,
        captured: &CapturedFile,
        stage: &StageCommitment,
        now: u64,
    ) -> Result<FileDeliveryReceipt, Error> {
        self.upload_transaction(
            || Ok(now),
            |tx, n| bind(tx, cap, id, preparation, captured, stage, n),
        )
    }
    pub fn claim_file_publication(
        &mut self,
        cap: &RunnerCapability,
        id: &FileDeliveryIdentity,
        now: u64,
        lease: u64,
    ) -> Result<Option<FilePublicationClaim>, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| claim(tx, cap, id, n, lease))
    }
    pub fn begin_file_publication(
        &mut self,
        cap: &RunnerCapability,
        claim: &FilePublicationClaim,
        now: u64,
    ) -> Result<FilePublicationSend, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| begin(tx, cap, claim, n))
    }
    pub fn validate_file_publication(
        &mut self,
        cap: &RunnerCapability,
        claim: &FilePublicationClaim,
        now: u64,
    ) -> Result<(), Error> {
        self.upload_transaction(|| Ok(now), |tx, n| validate(tx, cap, claim, n))
    }
    pub fn cancel_file_delivery(
        &mut self,
        id: &FileDeliveryIdentity,
        reason: FileDeliveryFailure,
        now: u64,
    ) -> Result<FileDeliveryReceipt, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| cancel(tx, id, reason, n))
    }
    pub fn mark_file_publication_uncertain(
        &mut self,
        id: &FileDeliveryIdentity,
        fence: u64,
        now: u64,
    ) -> Result<FileDeliveryReceipt, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| uncertain(tx, id, fence, n))
    }
    pub fn restore_file_delivery_settlement(
        &self,
        input: &FilePublicationLocator,
    ) -> Result<Option<FileDeliverySettlement>, Error> {
        lookup_input(input)?;
        let row = match read(&self.db, &input.delivery_id) {
            Ok(row) => row,
            Err(Error::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        };
        let settled = FileDeliverySettlement {
            identity: row.identity,
            locator: input.clone(),
        };
        settlement_row(&self.db, &settled)?;
        Ok(Some(settled))
    }
    pub fn inspect_file_delivery_settlement(
        &self,
        settlement: &FileDeliverySettlement,
    ) -> Result<FileDeliveryReceipt, Error> {
        settlement_row(&self.db, settlement)?.receipt(true)
    }
    /// Historical content association only. Returns no current publication authority.
    pub fn restore_file_delivery_settlement_for_content(
        &self,
        input: &FilePublicationLocator,
        request: &FileDeliveryRequest,
        captured: &CapturedFile,
    ) -> Result<Option<FileDeliverySettlement>, Error> {
        request.validate()?;
        captured.validate()?;
        let Some(settlement) = self.restore_file_delivery_settlement(input)? else {
            return Ok(None);
        };
        let row = settlement_row(&self.db, &settlement)?;
        if row.request != *request || row.captured.as_ref() != Some(captured) {
            return Err(Error::Conflict);
        }
        Ok(Some(settlement))
    }
    pub fn record_file_delivery_settlement(
        &mut self,
        settlement: &FileDeliverySettlement,
        observed: &FileDeliveryAcceptance,
        now: u64,
    ) -> Result<FileDeliveryReceipt, Error> {
        self.upload_transaction(|| Ok(now), |tx, n| settle(tx, settlement, observed, n))
    }
}
