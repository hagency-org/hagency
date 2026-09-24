//! Only this private SDK adapter combines secret staged metadata and accepted MXC.
use super::{
    Sdk,
    upload_state::{Context, Ledger, Phase as UploadPhase, Reference},
};
use crate::{
    Error,
    outgoing::state::{self, Attempt, Kind, Phase},
};
use hagency_core::file_delivery::{
    CapturedFile, FILE_MIME, FileDeliveryRequest, FilePublicationLocator,
};
use hagency_store::FilePublicationSend;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) struct Start {
    pub send: FilePublicationSend,
    pub reference: Reference,
    pub descriptor: Vec<u8>,
    pub joined: BTreeSet<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub locator: FilePublicationLocator,
    metadata: FileDeliveryRequest,
    captured: CapturedFile,
    descriptor: Vec<u8>,
}
impl Binding {
    pub(crate) fn metadata(&self) -> &FileDeliveryRequest {
        &self.metadata
    }
    pub(crate) fn captured(&self) -> &CapturedFile {
        &self.captured
    }
}

impl Sdk {
    pub(super) fn start_file(&self, start: Start) -> Result<Attempt, Error> {
        if self.upload_poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let locator = start.send.locator().clone();
        if start.reference.id() != locator.upload_id
            || start.reference.fence() != locator.upload_fence
            || start.reference.stage() != &locator.stage
            || start.reference.route() != &locator.route
        {
            return Err(Error::Conflict);
        }
        let binding = Binding {
            locator,
            metadata: start.send.metadata().clone(),
            captured: start.send.captured().clone(),
            descriptor: start.descriptor,
        };
        let content = content(&binding, self.uploads.as_ref(), &self.upload_context)?;
        let l = &binding.locator;
        Ok(Attempt {
            kind: Kind::File,
            id: l.delivery_id.clone(),
            fence: l.fence,
            domain_digest: l.content_digest.clone(),
            route: l.route.clone(),
            transaction_id: l.transaction_id.clone(),
            content_digest: state::hash(state::encode(&content, state::MAX_EVENT)?.as_bytes()),
            content,
            identity: String::new(),
            joined: start.joined,
            phase: Phase::BeforeBegin,
            query_id: None,
            query_body: None,
            query_response: None,
            keys_digest: None,
            writes: vec![],
            index: 0,
            file: Some(binding),
        })
    }
}

pub(super) fn validate_attempt(
    attempt: &Attempt,
    ledger: Option<&Ledger>,
    context: &Context,
) -> Result<(), Error> {
    let Some(binding) = &attempt.file else {
        return if attempt.kind == Kind::File {
            Err(Error::Storage)
        } else {
            Ok(())
        };
    };
    let l = &binding.locator;
    if attempt.kind != Kind::File
        || attempt.id != l.delivery_id
        || attempt.fence != l.fence
        || attempt.domain_digest != l.content_digest
        || attempt.route != l.route
        || attempt.transaction_id != l.transaction_id
        || attempt.content != content(binding, ledger, context)?
    {
        return Err(Error::Storage);
    }
    Ok(())
}

fn content(binding: &Binding, ledger: Option<&Ledger>, context: &Context) -> Result<Value, Error> {
    let l = &binding.locator;
    binding.metadata.validate().map_err(|_| Error::Storage)?;
    binding.captured.validate().map_err(|_| Error::Storage)?;
    if !l.route.encrypted || binding.captured.size != l.stage.len {
        return Err(Error::Storage);
    }
    let descriptor = hagency_media::Descriptor::from_private_event_json(&binding.descriptor)
        .map_err(|_| Error::Storage)?;
    let operation =
        hagency_media_store::OperationId::new(&l.stage.operation_id).map_err(|_| Error::Storage)?;
    let len = usize::try_from(l.stage.len).map_err(|_| Error::Storage)?;
    if !hagency_media_store::encrypted_receipt_matches(
        &digest_bytes(&l.stage.namespace_digest)?,
        &operation,
        len,
        &descriptor,
        &digest_bytes(&l.stage.receipt_digest)?,
    ) {
        return Err(Error::Storage);
    }
    let ledger = ledger.ok_or(Error::OutcomeUnknown)?;
    let reference = ledger.restore(&l.upload_id, context)?;
    if reference.fence() != l.upload_fence
        || reference.stage() != &l.stage
        || reference.route() != &l.route
    {
        return Err(Error::Conflict);
    }
    let record = ledger.exact(&reference)?;
    if record.phase != UploadPhase::Accepted {
        return Err(Error::OutcomeUnknown);
    }
    let response = record.response.as_ref().ok_or(Error::OutcomeUnknown)?;
    if response.receipt_id != l.upload_receipt_id
        || response.receipt_digest != l.upload_receipt_digest
    {
        return Err(Error::Conflict);
    }
    let mut file: Value =
        serde_json::from_slice(descriptor.private_event_json()).map_err(|_| Error::Storage)?;
    file["url"] = json!(response.mxc);
    let mut result = json!({"msgtype":"m.file", "body":binding.metadata.caption.as_ref().unwrap_or(&binding.metadata.filename),
        "file":file, "info":{"mimetype":FILE_MIME,"size":binding.captured.size}});
    if binding.metadata.caption.is_some() {
        result["filename"] = json!(binding.metadata.filename);
    }
    if let Some(root) = &l.route.thread_root {
        result["m.relates_to"] = json!({"rel_type":"m.thread","event_id":root,"is_falling_back":true,"m.in_reply_to":{"event_id":root}});
    }
    Ok(result)
}

fn digest_bytes(value: &str) -> Result<[u8; 32], Error> {
    hagency_core::uploads::digest(value).map_err(|_| Error::Storage)?;
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| Error::Storage)?;
    }
    Ok(bytes)
}

// Test-only coherent changes to a real completed protected snapshot. Recompute
// dependent content hashes so tests exercise original association, not a typo.
#[cfg(test)]
pub(super) fn corrupt(attempt: &mut Attempt, variant: u8) {
    let binding = attempt.file.as_mut().unwrap();
    match variant {
        10 => {
            binding.metadata.filename.push_str("-changed");
            if binding.metadata.caption.is_some() {
                attempt.content["filename"] = json!(binding.metadata.filename);
            } else {
                attempt.content["body"] = json!(binding.metadata.filename);
            }
        }
        11 => binding.captured.sha256 = "f".repeat(64),
        12 => {
            let mut descriptor: Value = serde_json::from_slice(&binding.descriptor).unwrap();
            let old = descriptor["key"]["k"].as_str().unwrap();
            let changed = format!(
                "{}{}",
                if old.starts_with('A') { "B" } else { "A" },
                &old[1..]
            );
            descriptor["key"]["k"] = json!(changed);
            binding.descriptor = serde_json::to_vec(&descriptor).unwrap();
            attempt.content["file"]["key"]["k"] = json!(changed);
        }
        13 => binding.locator.upload_receipt_digest = "e".repeat(64),
        _ => panic!("unknown file corruption"),
    }
    attempt.content_digest = state::hash(
        state::encode(&attempt.content, state::MAX_EVENT)
            .unwrap()
            .as_bytes(),
    );
}
