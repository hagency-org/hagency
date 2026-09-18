//! Fixed untrusted numbers, not source credentials or complete capture proof.
use crate::runtime_usage::ProjectionDiagnostics;
use crate::{MAX_TOKEN_COUNT, MeteringError, TokenCounts};
use serde::Serialize;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    MainLoopSteps,
    MainLoopResult,
    ReportedModelsResult,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClaudeUsage {
    pub counts: TokenCounts,
    pub coverage: Coverage,
    pub steps: u32,
    pub models: u16,
    pub diagnostics: ProjectionDiagnostics,
}
#[derive(Clone, Serialize)]
pub struct RuntimeEvidence {
    version: u8,
    stream_incomplete: bool,
    usage: ClaudeUsage,
}
impl RuntimeEvidence {
    pub fn version(&self) -> u8 {
        self.version
    }
    pub fn stream_incomplete(&self) -> bool {
        self.stream_incomplete
    }
    pub fn usage(&self) -> &ClaudeUsage {
        &self.usage
    }
}
pub(crate) fn normalize(
    mut usage: ClaudeUsage,
) -> Result<(TokenCounts, RuntimeEvidence), MeteringError> {
    if usage.steps > 1024 || usage.models > 64 {
        return Err(MeteringError::Capacity);
    }
    if usage.coverage == Coverage::MainLoopSteps {
        usage.counts.output = None;
    }
    if usage.coverage != Coverage::ReportedModelsResult && usage.models != 0 {
        return Err(MeteringError::InvalidRecord);
    }
    for value in [
        &mut usage.counts.input,
        &mut usage.counts.output,
        &mut usage.counts.cache_read,
        &mut usage.counts.cache_write,
    ] {
        if value.is_some_and(|n| n > MAX_TOKEN_COUNT) {
            *value = None;
            usage.diagnostics.invalid = true;
        }
        usage.diagnostics.missing |= value.is_none();
    }
    // Claude input is fresh input, not Codex's cache-inclusive input. Checking
    // all known categories also rejects overflow with another category absent.
    usage.counts.display_volume()?;
    Ok((
        usage.counts,
        RuntimeEvidence {
            version: 1,
            stream_incomplete: true,
            usage,
        },
    ))
}
