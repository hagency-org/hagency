use hagency_core::{
    attachments::{AttachmentMetadata, AttachmentPage},
    canonical,
    project::identifier,
    received_files::{MAX_RECEIVED_FILE_BYTES, ReceivedFileFacts},
    replies::matrix_event,
    tasks::RunnerCapability,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiveFile {
    pub event_id: String,
}
impl ReceiveFile {
    pub fn validate(&self) -> Result<(), ReceiveError> {
        matrix_event(&self.event_id).map_err(|_| ReceiveError::Invalid)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiveView {
    pub event_id: String,
    pub filename: String,
    pub mime_type: Option<String>,
    pub size: u64,
    pub sha256: String,
    pub path: String,
    pub replayed: bool,
}
impl ReceiveView {
    pub fn validate(&self) -> Result<(), ReceiveError> {
        matrix_event(&self.event_id).map_err(|_| ReceiveError::Invalid)?;
        AttachmentMetadata {
            filename: self.filename.clone(),
            mime_type: self.mime_type.clone(),
            declared_size: None,
        }
        .validate()
        .map_err(|_| ReceiveError::Invalid)?;
        ReceivedFileFacts {
            size: self.size,
            sha256: self.sha256.clone(),
        }
        .validate(MAX_RECEIVED_FILE_BYTES)
        .map_err(|_| ReceiveError::Invalid)?;
        let component = self
            .path
            .strip_prefix(".hagency-received-")
            .and_then(|v| v.strip_suffix(".bin"))
            .ok_or(ReceiveError::Invalid)?;
        if component.len() != 32
            || !component
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || serde_json::to_vec(self)
                .map_err(|_| ReceiveError::Invalid)?
                .len()
                > 4096
        {
            return Err(ReceiveError::Invalid);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiveError {
    Invalid,
    Unauthorized,
    Conflict,
    Busy,
    Unavailable,
    Unknown,
}
impl ReceiveError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Invalid => "invalid_receive_operation",
            Self::Unauthorized => "file_scope_required",
            Self::Conflict => "idempotency_conflict",
            Self::Busy => "busy",
            Self::Unavailable => "unavailable",
            Self::Unknown => "outcome_unknown",
        }
    }
}
impl From<hagency_store::Error> for ReceiveError {
    fn from(error: hagency_store::Error) -> Self {
        use hagency_store::Error;
        match error {
            Error::Invalid(_) => Self::Invalid,
            Error::RunnerAuthority | Error::NotFound => Self::Unauthorized,
            Error::Conflict => Self::Conflict,
            Error::Busy | Error::Capacity => Self::Busy,
            Error::OutcomeUnknown => Self::Unknown,
            _ => Self::Unavailable,
        }
    }
}
pub(super) fn key(cap: &RunnerCapability, event: &str) -> Result<String, ReceiveError> {
    identifier(&cap.dispatch_id, 128).map_err(|_| ReceiveError::Invalid)?;
    identifier(&cap.runner_id, 128).map_err(|_| ReceiveError::Invalid)?;
    hagency_core::uploads::digest(&cap.secret).map_err(|_| ReceiveError::Invalid)?;
    if cap.fence == 0 || cap.fence > hagency_core::JSON_SAFE_MAX {
        return Err(ReceiveError::Invalid);
    }
    matrix_event(event).map_err(|_| ReceiveError::Invalid)?;
    canonical::digest(&serde_json::json!(["receive_job_v1", cap, event]))
        .map_err(|_| ReceiveError::Invalid)
}
pub(super) fn page(value: &AttachmentPage, after: u64, limit: usize) -> Result<(), ReceiveError> {
    if value.items.len() > limit
        || value.items.len() > 16
        || serde_json::to_vec(value)
            .map_err(|_| ReceiveError::Unavailable)?
            .len()
            > 16 * 1024
    {
        return Err(ReceiveError::Unavailable);
    }
    let mut last = after;
    for item in &value.items {
        matrix_event(&item.event_id).map_err(|_| ReceiveError::Unavailable)?;
        item.metadata
            .validate()
            .map_err(|_| ReceiveError::Unavailable)?;
        if item.sequence <= last || item.sequence > hagency_core::JSON_SAFE_MAX {
            return Err(ReceiveError::Unavailable);
        }
        last = item.sequence;
    }
    if value
        .next
        .is_some_and(|next| next != last || value.items.is_empty())
    {
        return Err(ReceiveError::Unavailable);
    }
    Ok(())
}
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_receive_service_projection() {
        let valid = ReceiveView {
            event_id: "$original".into(),
            filename: "untrusted.txt".into(),
            mime_type: Some("text/plain".into()),
            size: 4,
            sha256: "a".repeat(64),
            path: format!(".hagency-received-{}.bin", "b".repeat(32)),
            replayed: false,
        };
        valid.validate().unwrap();
        for path in [
            "/tmp/a",
            "../a",
            ".hagency-received-ABC.bin",
            ".hagency-received-00000000000000000000000000000000.bin/evil",
        ] {
            let mut value = valid.clone();
            value.path = path.into();
            assert_eq!(value.validate(), Err(ReceiveError::Invalid));
        }
        let mut value = valid.clone();
        value.size = MAX_RECEIVED_FILE_BYTES as u64 + 1;
        assert!(value.validate().is_err());
        value = valid.clone();
        value.sha256 = "A".repeat(64);
        assert!(value.validate().is_err());
        value = valid;
        value.filename = "../user".into();
        assert!(value.validate().is_err());
        assert!(
            serde_json::from_str::<ReceiveFile>(r#"{"event_id":"$original","path":"x"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<ReceiveFile>(r#"{"event_id":"$original","event_id":"$other"}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<ReceiveView>(r#"{"event_id":"$original","root":"/private"}"#)
                .is_err()
        );
    }
}
