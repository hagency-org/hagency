//! Safe attachment metadata is data, never a descriptor or download authority.
use crate::{InvalidInput, ingress::MatrixEventObservation, tasks::clock};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentMetadata {
    pub filename: String,
    pub mime_type: Option<String>,
    pub declared_size: Option<u64>,
}
impl AttachmentMetadata {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        if self.filename.is_empty()
            || self.filename.len() > 255
            || matches!(self.filename.as_str(), "." | "..")
            || self
                .filename
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
        {
            return Err(InvalidInput("invalid attachment filename"));
        }
        if self.mime_type.as_ref().is_some_and(|v| {
            v.is_empty()
                || v.len() > 128
                || !v.is_ascii()
                || v.bytes()
                    .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
                || !v.contains('/')
        }) {
            return Err(InvalidInput("invalid attachment MIME type"));
        }
        if let Some(size) = self.declared_size {
            clock(size)?;
        }
        Ok(())
    }
}

/// Constructed only by the authenticated host collector. No runner/HTTP input
/// may deserialize it. The protected SDK journal owns all media secrets.
#[derive(Clone, Serialize)]
pub struct MatrixAttachmentObservation {
    pub event: MatrixEventObservation,
    pub metadata: AttachmentMetadata,
    pub sdk_identity: String,
    pub manifest_id: String,
    pub content_digest: String,
}
impl MatrixAttachmentObservation {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        self.event.validate()?;
        self.metadata.validate()?;
        if !self.event.encrypted || !matches!(self.event.event.kind.as_str(), "m.file" | "m.image")
        {
            return Err(InvalidInput(
                "attachment requires authenticated encrypted file event",
            ));
        }
        for value in [&self.sdk_identity, &self.manifest_id, &self.content_digest] {
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(InvalidInput("invalid attachment manifest identity"));
            }
        }
        Ok(())
    }
}

/// A current read projection. Metadata and cursor do not grant attachment access.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentSummary {
    pub event_id: String,
    pub sequence: u64,
    pub metadata: AttachmentMetadata,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentPage {
    pub items: Vec<AttachmentSummary>,
    pub next: Option<u64>,
}
