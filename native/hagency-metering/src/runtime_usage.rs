//! Fixed untrusted numerical input. These public values carry no runtime source,
//! execution, provider or capture-completeness authority. No raw payload is held.
use crate::{MAX_TOKEN_COUNT, MeteringError, TokenCounts};
use serde::Serialize;

#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize)]
pub struct CounterBreakdown {
    pub total_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize)]
pub struct ProjectionDiagnostics {
    pub missing: bool,
    pub invalid: bool,
    pub unsupported: bool,
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize)]
pub struct CodexUsage {
    pub total: CounterBreakdown,
    pub last: CounterBreakdown,
    pub context_window: Option<u64>,
    pub diagnostics: ProjectionDiagnostics,
}

/// Contradictions are retained per snapshot; cross-snapshot regression belongs
/// to the separately attributed high-water ledger, not this stateless converter.
#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize)]
pub struct NormalizationDiagnostics {
    pub total_inconsistent: bool,
    pub last_inconsistent: bool,
}

/// Sanitized evidence with a private constructor, not a provenance credential.
/// Incomplete stream coverage is never cleared by a complete numerical shape.
#[derive(Clone, Serialize)]
pub struct RuntimeEvidence {
    version: u8,
    stream_incomplete: bool,
    usage: CodexUsage,
    normalization: NormalizationDiagnostics,
}
impl RuntimeEvidence {
    pub fn version(&self) -> u8 {
        self.version
    }
    pub fn stream_incomplete(&self) -> bool {
        self.stream_incomplete
    }
    pub fn usage(&self) -> &CodexUsage {
        &self.usage
    }
    pub fn normalization(&self) -> NormalizationDiagnostics {
        self.normalization
    }
}

fn sanitize(value: &mut Option<u64>, diagnostics: &mut ProjectionDiagnostics) {
    if value.is_some_and(|value| value > MAX_TOKEN_COUNT) {
        *value = None;
        diagnostics.invalid = true;
    }
    diagnostics.missing |= value.is_none();
}

impl CounterBreakdown {
    fn sanitize(&mut self, diagnostics: &mut ProjectionDiagnostics) {
        for value in [
            &mut self.total_tokens,
            &mut self.input_tokens,
            &mut self.cached_input_tokens,
            &mut self.cache_write_input_tokens,
            &mut self.output_tokens,
            &mut self.reasoning_output_tokens,
        ] {
            sanitize(value, diagnostics);
        }
    }
    fn fresh(&self) -> Option<u64> {
        self.input_tokens?
            .checked_sub(self.cached_input_tokens?)?
            .checked_sub(self.cache_write_input_tokens?)
    }
    fn inconsistent(&self) -> bool {
        let excessive_cache = match (
            self.input_tokens,
            self.cached_input_tokens,
            self.cache_write_input_tokens,
        ) {
            (Some(input), read, write) => {
                // Even a missing category cannot conceal an independently
                // impossible known lower bound. Sanitization precedes arithmetic.
                read.unwrap_or(0) + write.unwrap_or(0) > input
            }
            _ => false,
        };
        let excessive_reasoning = self
            .reasoning_output_tokens
            .zip(self.output_tokens)
            .is_some_and(|(reasoning, output)| reasoning > output);
        let different_total = self
            .input_tokens
            .zip(self.output_tokens)
            .zip(self.total_tokens)
            .is_some_and(|((input, output), total)| input + output != total);
        excessive_cache || excessive_reasoning || different_total
    }
}

pub(crate) fn normalize(
    mut usage: CodexUsage,
) -> Result<(TokenCounts, RuntimeEvidence), MeteringError> {
    usage.total.sanitize(&mut usage.diagnostics);
    usage.last.sanitize(&mut usage.diagnostics);
    sanitize(&mut usage.context_window, &mut usage.diagnostics);
    let normalization = NormalizationDiagnostics {
        total_inconsistent: usage.total.inconsistent(),
        last_inconsistent: usage.last.inconsistent(),
    };
    let totals = TokenCounts {
        input: usage.total.fresh(),
        output: usage.total.output_tokens,
        cache_write: usage.total.cache_write_input_tokens,
        cache_read: usage.total.cached_input_tokens,
    };
    // Known subtotals are checked even if another field is unknown. Failure
    // returns no partial observation; the host must retain its original input.
    totals.display_volume()?;
    Ok((
        totals,
        RuntimeEvidence {
            version: 1,
            stream_incomplete: true,
            usage,
            normalization,
        },
    ))
}
