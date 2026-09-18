//! Numeric projection only; no provider, task, source or billing authority.
use serde_json::Value;
use std::collections::BTreeMap;
const MAX_COUNT: u64 = 9_007_199_254_740_991;
const MAX_STEPS: usize = 1024;
const MAX_MODELS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UsageCoverage {
    MainLoopSteps,
    MainLoopResult,
    ReportedModelsResult,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageDiagnostics {
    missing: bool,
    invalid: bool,
    unsupported: bool,
}
impl UsageDiagnostics {
    pub fn has_missing_fields(self) -> bool {
        self.missing
    }
    pub fn has_invalid_fields(self) -> bool {
        self.invalid
    }
    pub fn has_unsupported_fields(self) -> bool {
        self.unsupported
    }
    fn merge(&mut self, other: Self) {
        self.missing |= other.missing;
        self.invalid |= other.invalid;
        self.unsupported |= other.unsupported;
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageCounts {
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
}
impl UsageCounts {
    pub fn input(self) -> Option<u64> {
        self.input
    }
    pub fn output(self) -> Option<u64> {
        self.output
    }
    pub fn cache_read(self) -> Option<u64> {
        self.cache_read
    }
    pub fn cache_write(self) -> Option<u64> {
        self.cache_write
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct UsageEvidence {
    counts: UsageCounts,
    coverage: UsageCoverage,
    diagnostics: UsageDiagnostics,
    steps: u32,
    models: u16,
}
impl UsageEvidence {
    pub fn counts(&self) -> UsageCounts {
        self.counts
    }
    pub fn coverage(&self) -> UsageCoverage {
        self.coverage
    }
    pub fn diagnostics(&self) -> UsageDiagnostics {
        self.diagnostics
    }
    pub fn steps(&self) -> u32 {
        self.steps
    }
    pub fn models(&self) -> u16 {
        self.models
    }
}
fn counter(value: Option<&Value>, diagnostics: &mut UsageDiagnostics) -> Option<u64> {
    match value {
        None | Some(Value::Null) => {
            diagnostics.missing = true;
            None
        }
        Some(value) => match value.as_u64().filter(|v| *v <= MAX_COUNT) {
            Some(v) => Some(v),
            None => {
                diagnostics.invalid = true;
                None
            }
        },
    }
}
fn project(value: &Value, model: bool, step: bool) -> (UsageCounts, UsageDiagnostics) {
    let keys = if model {
        [
            "inputTokens",
            "outputTokens",
            "cacheReadInputTokens",
            "cacheCreationInputTokens",
        ]
    } else {
        [
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ]
    };
    let mut d = UsageDiagnostics::default();
    if !value.is_object() && !value.is_null() {
        d.invalid = true;
    }
    if let Some(object) = value.as_object() {
        d.unsupported = object.keys().any(|k| !keys.contains(&k.as_str()));
    }
    let counts = UsageCounts {
        input: counter(value.get(keys[0]), &mut d),
        // Placeholder output never participates in duplicate identity or sums.
        output: if step {
            d.missing = true;
            None
        } else {
            counter(value.get(keys[1]), &mut d)
        },
        cache_read: counter(value.get(keys[2]), &mut d),
        cache_write: counter(value.get(keys[3]), &mut d),
    };
    (counts, d)
}
#[derive(Default)]
struct Sum {
    known: u64,
    missing: bool,
}
impl Sum {
    fn add(&mut self, value: Option<u64>) -> Result<(), ()> {
        match value {
            Some(value) => {
                self.known = self
                    .known
                    .checked_add(value)
                    .filter(|n| *n <= MAX_COUNT)
                    .ok_or(())?
            }
            None => self.missing = true,
        };
        Ok(())
    }
    fn value(&self) -> Option<u64> {
        (!self.missing).then_some(self.known)
    }
}
#[derive(Default)]
struct Totals {
    input: Sum,
    output: Sum,
    read: Sum,
    write: Sum,
}
impl Totals {
    fn add(&mut self, c: UsageCounts) -> Result<(), ()> {
        self.input.add(c.input)?;
        self.output.add(c.output)?;
        self.read.add(c.cache_read)?;
        self.write.add(c.cache_write)?;
        // Unknown fields must not hide known combined overflow either.
        self.input
            .known
            .checked_add(self.output.known)
            .and_then(|n| n.checked_add(self.read.known))
            .and_then(|n| n.checked_add(self.write.known))
            .filter(|n| *n <= MAX_COUNT)
            .ok_or(())?;
        Ok(())
    }
    fn counts(&self) -> UsageCounts {
        UsageCounts {
            input: self.input.value(),
            output: self.output.value(),
            cache_read: self.read.value(),
            cache_write: self.write.value(),
        }
    }
}
#[derive(Default)]
pub(super) struct Tracker {
    seen: BTreeMap<String, (UsageCounts, UsageDiagnostics)>,
    totals: Totals,
    diagnostics: UsageDiagnostics,
    invalidated: bool,
}
impl Tracker {
    pub(super) fn project(
        &mut self,
        kind: super::EventKind,
        payload: &Value,
    ) -> super::ObservationKind {
        use super::ObservationKind as Kind;
        if self.invalidated {
            return Kind::Invalidated;
        }
        let result = match kind {
            super::EventKind::Assistant => self
                .step(payload)
                .map(|e| e.map_or(Kind::Ignored, Kind::Usage)),
            super::EventKind::Result => self.result(payload).map(|usage| Kind::Result {
                usage,
                is_error: payload["is_error"] == true,
            }),
            _ => Ok(Kind::Ignored),
        };
        result.unwrap_or_else(|()| {
            self.invalidated = true;
            Kind::Invalidated
        })
    }
    fn step(&mut self, payload: &Value) -> Result<Option<UsageEvidence>, ()> {
        // A nested agent is not part of the main-loop partial sum.
        if let Some(parent) = payload.get("parent_tool_use_id").filter(|v| !v.is_null()) {
            return if parent.as_str().is_some_and(|s| !s.is_empty()) {
                Ok(None)
            } else {
                Err(())
            };
        }
        let message = &payload["message"];
        let id = message["id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .ok_or(())?;
        let (counts, diagnostics) = project(&message["usage"], false, true);
        if let Some(original) = self.seen.get(id) {
            return if original == &(counts, diagnostics) {
                Ok(None)
            } else {
                Err(())
            };
        }
        if self.seen.len() >= MAX_STEPS {
            return Err(());
        }
        self.totals.add(counts)?;
        self.diagnostics.merge(diagnostics);
        self.seen.insert(id.into(), (counts, diagnostics));
        Ok(Some(UsageEvidence {
            counts: self.totals.counts(),
            coverage: UsageCoverage::MainLoopSteps,
            diagnostics: self.diagnostics,
            steps: self.seen.len() as u32,
            models: 0,
        }))
    }
    fn result(&self, payload: &Value) -> Result<UsageEvidence, ()> {
        let (counts, coverage, diagnostics, models) =
            match payload.get("modelUsage").filter(|v| !v.is_null()) {
                Some(value) => {
                    let Some(models) = value.as_object().filter(|m| !m.is_empty()) else {
                        return Ok(UsageEvidence {
                            counts: UsageCounts::default(),
                            coverage: UsageCoverage::ReportedModelsResult,
                            diagnostics: UsageDiagnostics {
                                missing: true,
                                invalid: true,
                                unsupported: false,
                            },
                            steps: self.seen.len() as u32,
                            models: 0,
                        });
                    };
                    if models.len() > MAX_MODELS {
                        return Err(());
                    }
                    let mut totals = Totals::default();
                    let mut d = UsageDiagnostics::default();
                    for value in models.values() {
                        let (c, next) = project(value, true, false);
                        totals.add(c)?;
                        d.merge(next);
                    }
                    (
                        totals.counts(),
                        UsageCoverage::ReportedModelsResult,
                        d,
                        models.len() as u16,
                    )
                }
                None => {
                    let (counts, d) = project(&payload["usage"], false, false);
                    let mut totals = Totals::default();
                    totals.add(counts)?;
                    (counts, UsageCoverage::MainLoopResult, d, 0)
                }
            };
        Ok(UsageEvidence {
            counts,
            coverage,
            diagnostics,
            steps: self.seen.len() as u32,
            models,
        })
    }
}
