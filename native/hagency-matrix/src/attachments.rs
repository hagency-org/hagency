//! Private authenticated manifest custody, not a runtime download authority.
use crate::{CancellationToken, Collector, Error, MediaId, sdk::Owner};
use hagency_core::{
    attachments::{AttachmentMetadata, MatrixAttachmentObservation},
    canonical,
    ingress::{MatrixEventObservation, MatrixIngressScope},
    replies::ReplyRoute,
    tasks::RunnerCapability,
};
use hagency_media::Descriptor;
use hagency_store::AttachmentTicket;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::OwnedSemaphorePermit;

pub(crate) const MAX_MANIFESTS: usize = 128;
pub(crate) const MAX_MANIFEST_BYTES: usize = 16 * 1024;
pub(crate) const MAX_HANDLES: usize = 8;
fn digest(value: &Value) -> Result<String, Error> {
    canonical::transport_digest(value).map_err(|_| Error::Wire)
}
pub(crate) fn identity(value: &str) -> Result<String, Error> {
    digest(&json!(["hagency.matrix.attachment.sdk.v1", value]))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub id: String,
    pub sdk_identity: String,
    pub content_digest: String,
    pub route: ReplyRoute,
    source: Value,
    content: Value,
    sender_device: String,
    session: String,
}
fn parts(content: &Value) -> Result<(AttachmentMetadata, MediaId, Descriptor), Error> {
    let object = content.as_object().ok_or(Error::Wire)?;
    if !matches!(
        object.get("msgtype").and_then(Value::as_str),
        Some("m.file" | "m.image")
    ) || object.contains_key("url")
    {
        return Err(Error::Unsupported);
    }
    let mut file = object
        .get("file")
        .and_then(Value::as_object)
        .cloned()
        .ok_or(Error::Wire)?;
    let url = file
        .remove("url")
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or(Error::Wire)?;
    let media = MediaId::new(&url).map_err(|_| Error::Wire)?;
    let descriptor =
        Descriptor::from_private_event_json(&serde_json::to_vec(&file).map_err(|_| Error::Wire)?)
            .map_err(|_| Error::Wire)?;
    let filename = object
        .get("filename")
        .or_else(|| object.get("body"))
        .and_then(Value::as_str)
        .ok_or(Error::Wire)?
        .to_owned();
    let info = object
        .get("info")
        .map(|v| v.as_object().ok_or(Error::Wire))
        .transpose()?;
    let mime_type = info
        .and_then(|v| v.get("mimetype"))
        .map(|v| v.as_str().map(str::to_owned).ok_or(Error::Wire))
        .transpose()?;
    let declared_size = info
        .and_then(|v| v.get("size"))
        .map(|v| v.as_u64().ok_or(Error::Wire))
        .transpose()?;
    let metadata = AttachmentMetadata {
        filename,
        mime_type,
        declared_size,
    };
    metadata.validate().map_err(|_| Error::Wire)?;
    Ok((metadata, media, descriptor))
}
impl Manifest {
    pub(crate) fn new(
        sdk: &str,
        route: &ReplyRoute,
        original: &Value,
        content: &Value,
        sender_device: &str,
        session: &str,
    ) -> Result<Self, Error> {
        let mut source = original.clone();
        source
            .as_object_mut()
            .ok_or(Error::Wire)?
            .remove("unsigned");
        let sdk_identity = identity(sdk)?;
        // Identical authenticated source content remains equal across recipients.
        // SDK receiver identity, route and verification-device evidence are separate.
        let content_digest = digest(&json!([
            "hagency.matrix.attachment.content.v1",
            route.server_name,
            route.room_id,
            source,
            content,
        ]))?;
        let id = digest(&json!([
            "hagency.matrix.attachment.manifest.v1",
            sdk_identity,
            route,
            content_digest,
            sender_device,
            session,
        ]))?;
        let value = Self {
            id,
            sdk_identity,
            content_digest,
            route: route.clone(),
            source,
            content: content.clone(),
            sender_device: sender_device.into(),
            session: session.into(),
        };
        value.validate(sdk, &route.sender_mxid, &route.device_id)?;
        Ok(value)
    }
    pub(crate) fn validate(&self, sdk: &str, user: &str, device: &str) -> Result<(), Error> {
        if serde_json::to_vec(self).map_err(|_| Error::Storage)?.len() > MAX_MANIFEST_BYTES
            || !self.route.encrypted
            || self.route.sender_mxid != user
            || self.route.device_id != device
            || self.sender_device.is_empty()
            || self.sender_device.len() > 255
            || self.session.is_empty()
            || self.session.len() > 255
            || self.source.get("type").and_then(Value::as_str) != Some("m.room.encrypted")
            || self.source.get("unsigned").is_some()
            || self.sdk_identity != identity(sdk)?
            || self.content_digest
                != digest(&json!([
                    "hagency.matrix.attachment.content.v1",
                    self.route.server_name,
                    self.route.room_id,
                    self.source,
                    self.content,
                ]))?
            || self.id
                != digest(&json!([
                    "hagency.matrix.attachment.manifest.v1",
                    self.sdk_identity,
                    self.route,
                    self.content_digest,
                    self.sender_device,
                    self.session,
                ]))?
        {
            return Err(Error::Storage);
        }
        MatrixIngressScope::from(&self.route)
            .validate()
            .map_err(|_| Error::Storage)?;
        for (field, parse) in [("event_id", true), ("sender", false)] {
            let value = self
                .source
                .get(field)
                .and_then(Value::as_str)
                .ok_or(Error::Storage)?;
            if (parse && ruma::EventId::parse(value).is_err())
                || (!parse && ruma::UserId::parse(value).is_err())
            {
                return Err(Error::Storage);
            }
        }
        parts(&self.content)?;
        Ok(())
    }
    pub(crate) fn matches_original(&self, original: &Value) -> bool {
        let mut original = original.clone();
        if let Some(object) = original.as_object_mut() {
            object.remove("unsigned");
        }
        self.source == original
    }
    pub(crate) fn observation(
        &self,
        event: MatrixEventObservation,
    ) -> Result<MatrixAttachmentObservation, Error> {
        if self.source.get("event_id").and_then(Value::as_str)
            != Some(event.event.event_id.as_str())
            || self.source.get("sender").and_then(Value::as_str)
                != Some(event.event.sender_mxid.as_str())
            || self.source.get("origin_server_ts").and_then(Value::as_u64)
                != Some(event.event.origin_ts)
            || self.content.get("body").and_then(Value::as_str) != Some(event.event.body.as_str())
            || self.content.get("msgtype").and_then(Value::as_str)
                != Some(event.event.kind.as_str())
            || !event.scope.matches(&self.route)
            || !event.encrypted
        {
            return Err(Error::Conflict);
        }
        let result = MatrixAttachmentObservation {
            event,
            metadata: parts(&self.content)?.0,
            sdk_identity: self.sdk_identity.clone(),
            manifest_id: self.id.clone(),
            content_digest: self.content_digest.clone(),
        };
        result.validate().map_err(|_| Error::Wire)?;
        Ok(result)
    }
    pub(crate) fn matches(&self, ticket: &AttachmentTicket) -> bool {
        self.id == ticket.manifest_id()
            && self.sdk_identity == ticket.sdk_identity()
            && self.content_digest == ticket.content_digest()
            && ticket.source_scope().matches(&self.route)
            && self.source.get("event_id").and_then(Value::as_str) == Some(ticket.event_id())
            && parts(&self.content).is_ok_and(|v| &v.0 == ticket.metadata())
    }
    pub(crate) fn handle(self, permit: OwnedSemaphorePermit) -> Result<AttachmentHandle, Error> {
        let (_, media, descriptor) = parts(&self.content)?;
        Ok(AttachmentHandle {
            _manifest: self,
            media,
            descriptor,
            _permit: permit,
        })
    }
}

/// Borrowed secrets for a trusted host adapter only. This remains readable after
/// later revocation; a future receive operation MUST revalidate its capability
/// after download before exposing bytes. No Debug/Serialize/Clone/constructor.
pub struct AttachmentHandle {
    _manifest: Manifest,
    media: MediaId,
    descriptor: Descriptor,
    _permit: OwnedSemaphorePermit,
}
impl AttachmentHandle {
    pub fn media_id(&self) -> &MediaId {
        &self.media
    }
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
}
impl Collector {
    pub async fn attachment_manifest(
        &self,
        cap: RunnerCapability,
        ticket: AttachmentTicket,
        cancel: &CancellationToken,
    ) -> Result<AttachmentHandle, Error> {
        let deadline = tokio::time::Instant::now() + self.inner.config.limits.sdk;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(Error::Cancelled),
            _ = tokio::time::sleep_until(deadline) => Err(Error::Timeout),
            result = self.lookup_attachment(cap, ticket, cancel) => {
                if cancel.is_cancelled() { Err(Error::Cancelled) }
                else if tokio::time::Instant::now() >= deadline { Err(Error::Timeout) }
                else { result }
            }
        }
    }
    async fn lookup_attachment(
        &self,
        cap: RunnerCapability,
        ticket: AttachmentTicket,
        cancel: &CancellationToken,
    ) -> Result<AttachmentHandle, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let _busy = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let permit = self
            .inner
            .attachment_handles
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Capacity)?;
        self.inner
            .domain
            .revalidate_attachment(cap.clone(), ticket.clone())
            .await?;
        let mut owner = self.inner.owner.lock().await;
        if owner.is_none() {
            *owner = Some(Owner::open_existing(&self.inner.config).await?);
        }
        let handle = owner
            .as_ref()
            .ok_or(Error::Storage)?
            .attachment(ticket.clone(), permit)
            .await?;
        #[cfg(test)]
        if self
            .inner
            .handoff_fault
            .load(std::sync::atomic::Ordering::SeqCst)
            == 6
        {
            self.inner.handoff_reached.notify_one();
            self.inner.handoff_continue.notified().await;
        }
        self.inner.domain.revalidate_attachment(cap, ticket).await?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(handle)
    }
}
