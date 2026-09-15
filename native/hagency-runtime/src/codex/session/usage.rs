//! Received counters only. Normalization and execution attribution belong above
//! runtime; these values do not prove provider billing or arithmetic consistency.
use serde_json::{Map, Value};

const MAX_COUNTER: u64 = 9_007_199_254_740_991;
const COUNTERS: [&str; 6] = [
    "totalTokens",
    "inputTokens",
    "cachedInputTokens",
    "cacheWriteInputTokens",
    "outputTokens",
    "reasoningOutputTokens",
];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageDiagnostics {
    missing: bool,
    invalid: bool,
    unsupported: bool,
}
impl UsageDiagnostics {
    pub fn has_missing_fields(&self) -> bool {
        self.missing
    }
    pub fn has_invalid_fields(&self) -> bool {
        self.invalid
    }
    pub fn has_unsupported_fields(&self) -> bool {
        self.unsupported
    }
}

/// Fixed optional upstream counters, not normalized or additive categories.
#[derive(Clone, PartialEq, Eq)]
pub struct UsageBreakdown([Option<u64>; 6]);
impl UsageBreakdown {
    pub fn total_tokens(&self) -> Option<u64> {
        self.0[0]
    }
    pub fn input_tokens(&self) -> Option<u64> {
        self.0[1]
    }
    pub fn cached_input_tokens(&self) -> Option<u64> {
        self.0[2]
    }
    pub fn cache_write_input_tokens(&self) -> Option<u64> {
        self.0[3]
    }
    pub fn output_tokens(&self) -> Option<u64> {
        self.0[4]
    }
    pub fn reasoning_output_tokens(&self) -> Option<u64> {
        self.0[5]
    }
}

/// Created only by the real scoped reader. No raw text, public constructor,
/// Deserialize, Serialize or Debug. See Observation for source and sequence.
#[derive(Clone, PartialEq, Eq)]
pub struct UsageEvidence {
    total: UsageBreakdown,
    last: UsageBreakdown,
    context_window: Option<u64>,
    diagnostics: UsageDiagnostics,
}
impl UsageEvidence {
    pub fn total(&self) -> &UsageBreakdown {
        &self.total
    }
    pub fn last(&self) -> &UsageBreakdown {
        &self.last
    }
    /// Model capacity, not consumed tokens.
    pub fn model_context_window(&self) -> Option<u64> {
        self.context_window
    }
    pub fn diagnostics(&self) -> UsageDiagnostics {
        self.diagnostics
    }
}

fn object<'a>(
    value: Option<&'a Value>,
    allowed: &[&str],
    diagnostics: &mut UsageDiagnostics,
) -> Option<&'a Map<String, Value>> {
    match value {
        Some(Value::Object(fields)) => {
            diagnostics.unsupported |= fields.keys().any(|key| !allowed.contains(&key.as_str()));
            Some(fields)
        }
        None | Some(Value::Null) => {
            diagnostics.missing = true;
            None
        }
        _ => {
            diagnostics.invalid = true;
            None
        }
    }
}
fn counter(value: Option<&Value>, diagnostics: &mut UsageDiagnostics) -> Option<u64> {
    match value {
        None | Some(Value::Null) => {
            diagnostics.missing = true;
            None
        }
        Some(value) => match value.as_u64().filter(|&n| n <= MAX_COUNTER) {
            Some(n) => Some(n),
            None => {
                diagnostics.invalid = true;
                None
            }
        },
    }
}
fn breakdown(value: Option<&Value>, diagnostics: &mut UsageDiagnostics) -> UsageBreakdown {
    let fields = object(value, &COUNTERS, diagnostics);
    UsageBreakdown(COUNTERS.map(|key| counter(fields.and_then(|v| v.get(key)), diagnostics)))
}
pub(super) fn project(params: &Value) -> UsageEvidence {
    let mut diagnostics = UsageDiagnostics::default();
    let params = object(
        Some(params),
        &["threadId", "turnId", "tokenUsage"],
        &mut diagnostics,
    );
    let usage = object(
        params.and_then(|v| v.get("tokenUsage")),
        &["total", "last", "modelContextWindow"],
        &mut diagnostics,
    );
    let total = breakdown(usage.and_then(|v| v.get("total")), &mut diagnostics);
    let last = breakdown(usage.and_then(|v| v.get("last")), &mut diagnostics);
    let context_window = counter(
        usage.and_then(|v| v.get("modelContextWindow")),
        &mut diagnostics,
    );
    UsageEvidence {
        total,
        last,
        context_window,
        diagnostics,
    }
}
