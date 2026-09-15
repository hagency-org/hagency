//! Bounded, untrusted transcript normalization. Reports do not establish Agent
//! attribution, provider authenticity, quota availability or task completion.

pub mod attribution;
mod json;
pub mod observation;
pub mod reader;
pub mod runtime_usage;

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 64 * 1024;
pub const MAX_LINES: usize = 16_384;
pub const MAX_IDENTITIES: usize = 4096;
pub const MAX_TOKEN_COUNT: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework {
    Claude,
    Codex,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MeteringError {
    #[error("transcript exceeds a normalization capacity limit")]
    Capacity,
    #[error("transcript contains duplicate JSON keys")]
    DuplicateKey,
    #[error("usage contains an invalid counter")]
    InvalidCounter,
    #[error("usage contains an unsupported record shape")]
    InvalidRecord,
    #[error("a repeated message identity has conflicting usage")]
    ConflictingIdentity,
    #[error("normalized token arithmetic exceeds the exact supported range")]
    Overflow,
}

/// Null is unknown, including when a contributing record omitted that field.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCounts {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_write: Option<u64>,
    pub cache_read: Option<u64>,
}

impl TokenCounts {
    pub fn display_volume(&self) -> Result<Option<u64>, MeteringError> {
        sum(&[self.input, self.output, self.cache_write, self.cache_read])
    }

    pub fn ceiling_volume(&self) -> Result<Option<u64>, MeteringError> {
        sum(&[self.input, self.output, self.cache_write])
    }

    fn add(self, next: Self) -> Result<Self, MeteringError> {
        Ok(Self {
            input: sum(&[self.input, next.input])?,
            output: sum(&[self.output, next.output])?,
            cache_write: sum(&[self.cache_write, next.cache_write])?,
            cache_read: sum(&[self.cache_read, next.cache_read])?,
        })
    }
}

fn sum(values: &[Option<u64>]) -> Result<Option<u64>, MeteringError> {
    let mut total = 0;
    let mut known = true;
    for value in values {
        if let Some(value) = value {
            total = add(total, *value)?;
        } else {
            known = false;
        }
    }
    Ok(known.then_some(total))
}

fn add(a: u64, b: u64) -> Result<u64, MeteringError> {
    a.checked_add(b)
        .filter(|total| *total <= MAX_TOKEN_COUNT)
        .ok_or(MeteringError::Overflow)
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub malformed_lines: u32,
    pub missing_usage_records: u32,
    pub missing_fields: u32,
    pub ambiguous_workspace: bool,
    pub undeduplicable_messages: u32,
    pub non_monotonic: u32,
    pub inconsistent_records: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReport {
    pub framework: Framework,
    /// Private untrusted source metadata. Never use as a filesystem capability.
    pub workspace_hint: Option<String>,
    pub totals: Option<TokenCounts>,
    pub details: SessionDetails,
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Serialize)]
#[serde(tag = "framework", rename_all = "lowercase")]
pub enum SessionDetails {
    Claude {
        messages: u32,
        models: Vec<String>,
    },
    Codex {
        turns: Option<u32>,
        #[serde(rename = "reasoningOutput")]
        reasoning_output: Option<u64>,
        #[serde(rename = "cumulativeTotal")]
        cumulative_total: Option<u64>,
        #[serde(rename = "agreesWithCli")]
        agrees_with_cli: Option<bool>,
    },
}

#[derive(Default)]
struct Parser {
    diagnostics: Diagnostics,
    workspaces: BTreeSet<String>,
    models: BTreeSet<String>,
    seen: BTreeMap<String, TokenCounts>,
    totals: Option<TokenCounts>,
    messages: u32,
    turns: u32,
    turns_known: bool,
    previous_total: Option<u64>,
    reasoning: Option<u64>,
    cumulative: Option<u64>,
    agrees: Option<bool>,
    known_claude_volume: u64,
    previous_usage: Option<[Option<u64>; 5]>,
}

/// Parse only the supplied snapshot; capacity/shape errors return no partial report.
pub fn parse_session(framework: Framework, snapshot: &str) -> Result<SessionReport, MeteringError> {
    if snapshot.len() > MAX_SNAPSHOT_BYTES {
        return Err(MeteringError::Capacity);
    }
    let mut parser = Parser {
        turns_known: true,
        ..Parser::default()
    };
    for (index, line) in snapshot.split('\n').enumerate() {
        if index >= MAX_LINES || line.len() > MAX_LINE_BYTES {
            return Err(MeteringError::Capacity);
        }
        if line.trim().is_empty() {
            continue;
        }
        let value = match serde_json::from_str::<json::UniqueValue>(line) {
            Ok(value) => value.0,
            Err(error) => {
                if error
                    .to_string()
                    .starts_with("duplicate transcript JSON key")
                {
                    return Err(MeteringError::DuplicateKey);
                }
                if error.to_string().starts_with("recursion limit exceeded") {
                    return Err(MeteringError::Capacity);
                }
                if error.to_string().starts_with("number out of range") {
                    return Err(MeteringError::InvalidCounter);
                }
                parser.diagnostics.malformed_lines += 1;
                continue;
            }
        };
        if !json::within_shape(&value) {
            return Err(MeteringError::Capacity);
        }
        let Some(record) = value.as_object() else {
            parser.diagnostics.malformed_lines += 1;
            continue;
        };
        match framework {
            Framework::Claude => parser.claude(record)?,
            Framework::Codex => parser.codex(record)?,
        }
    }
    if let Some(totals) = parser.totals {
        totals.display_volume()?;
    }
    parser.diagnostics.ambiguous_workspace = parser.workspaces.len() > 1;
    let workspace_hint = (parser.workspaces.len() == 1)
        .then(|| parser.workspaces.into_iter().next())
        .flatten();
    let details = match framework {
        Framework::Claude => {
            // JavaScript sorts strings by UTF-16 code units, not Unicode scalars.
            let mut models: Vec<_> = parser.models.into_iter().collect();
            models.sort_by_cached_key(|model| model.encode_utf16().collect::<Vec<_>>());
            SessionDetails::Claude {
                messages: parser.messages,
                models,
            }
        }
        Framework::Codex => SessionDetails::Codex {
            turns: (parser.totals.is_some() && parser.turns_known).then_some(parser.turns),
            reasoning_output: parser.reasoning,
            cumulative_total: parser.cumulative,
            agrees_with_cli: parser.agrees,
        },
    };
    Ok(SessionReport {
        framework,
        workspace_hint,
        totals: parser.totals,
        details,
        diagnostics: parser.diagnostics,
    })
}

fn hint(value: Option<&Value>, max: usize) -> Result<Option<&str>, MeteringError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.len() <= max => Ok(Some(value)),
        Some(Value::String(_)) => Err(MeteringError::Capacity),
        Some(_) => Err(MeteringError::InvalidRecord),
    }
}

fn object(value: Option<&Value>) -> Result<Option<&Map<String, Value>>, MeteringError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        Some(_) => Err(MeteringError::InvalidRecord),
    }
}

impl Parser {
    fn counter(
        &mut self,
        object: &Map<String, Value>,
        key: &str,
    ) -> Result<Option<u64>, MeteringError> {
        match object.get(key) {
            None | Some(Value::Null) => {
                self.diagnostics.missing_fields += 1;
                Ok(None)
            }
            Some(value) => value
                .as_u64()
                .filter(|v| *v <= MAX_TOKEN_COUNT)
                .map(Some)
                .ok_or(MeteringError::InvalidCounter),
        }
    }

    fn workspace(&mut self, value: Option<&Value>) -> Result<(), MeteringError> {
        if let Some(value) = hint(value, 4096)?.filter(|v| !v.is_empty()) {
            self.workspaces.insert(value.to_owned());
            if self.workspaces.len() > 16 {
                return Err(MeteringError::Capacity);
            }
        }
        Ok(())
    }

    fn claude(&mut self, record: &Map<String, Value>) -> Result<(), MeteringError> {
        self.workspace(record.get("cwd"))?;
        let assistant = record.get("type").and_then(Value::as_str) == Some("assistant")
            || record
                .get("message")
                .and_then(|message| message.get("role"))
                .and_then(Value::as_str)
                == Some("assistant");
        let Some(message) = object(record.get("message"))? else {
            self.diagnostics.missing_usage_records += u32::from(assistant);
            return Ok(());
        };
        let Some(usage) = object(message.get("usage"))? else {
            self.diagnostics.missing_usage_records += u32::from(assistant);
            return Ok(());
        };
        let counts = TokenCounts {
            input: self.counter(usage, "input_tokens")?,
            output: self.counter(usage, "output_tokens")?,
            cache_write: self.counter(usage, "cache_creation_input_tokens")?,
            cache_read: self.counter(usage, "cache_read_input_tokens")?,
        };
        if let Some(id) = hint(record.get("uuid"), 256)?.filter(|v| !v.is_empty()) {
            if let Some(previous) = self.seen.get(id) {
                return if previous == &counts {
                    Ok(())
                } else {
                    Err(MeteringError::ConflictingIdentity)
                };
            }
            if self.seen.len() >= MAX_IDENTITIES {
                return Err(MeteringError::Capacity);
            }
            self.seen.insert(id.to_owned(), counts);
        } else {
            self.diagnostics.undeduplicable_messages += 1;
        }
        // Track the known lower bound separately: an omitted field must not
        // erase earlier known values and thereby hide arithmetic overflow.
        for count in [
            counts.input,
            counts.output,
            counts.cache_write,
            counts.cache_read,
        ]
        .into_iter()
        .flatten()
        {
            self.known_claude_volume = add(self.known_claude_volume, count)?;
        }
        if let Some(model) = hint(message.get("model"), 256)? {
            self.models.insert(model.to_owned());
            if self.models.len() > 64 {
                return Err(MeteringError::Capacity);
            }
        }
        self.totals = Some(match self.totals {
            Some(previous) => previous.add(counts)?,
            None => counts,
        });
        self.messages += 1;
        Ok(())
    }

    fn codex(&mut self, record: &Map<String, Value>) -> Result<(), MeteringError> {
        let Some(payload) = object(record.get("payload"))? else {
            return Ok(());
        };
        self.workspace(payload.get("cwd"))?;
        let token_record = payload.get("type").and_then(Value::as_str) == Some("token_count");
        let Some(info) = object(payload.get("info"))? else {
            self.diagnostics.missing_usage_records += u32::from(token_record);
            return Ok(());
        };
        let Some(usage) = object(info.get("total_token_usage"))? else {
            self.diagnostics.missing_usage_records += u32::from(token_record);
            return Ok(());
        };
        let total = self.counter(usage, "total_tokens")?;
        let input = self.counter(usage, "input_tokens")?;
        let cached = self.counter(usage, "cached_input_tokens")?;
        let output = self.counter(usage, "output_tokens")?;
        let reasoning = self.counter(usage, "reasoning_output_tokens")?;
        if let Some(total) = total {
            if Some(total) != self.previous_total {
                self.turns += 1;
            }
            if self.previous_total.is_some_and(|previous| total < previous) {
                self.diagnostics.non_monotonic += 1;
            }
            self.previous_total = Some(total);
        } else {
            self.turns_known = false;
        }
        let normalized_input = input
            .zip(cached)
            .and_then(|(input, cached)| input.checked_sub(cached));
        let agrees = sum(&[input, output])?
            .zip(total)
            .map(|(sum, total)| sum == total);
        let components = [input, cached, output, reasoning, total];
        let contradicts_previous = self.previous_usage.is_some_and(|previous| {
            let same_total_changed_breakdown = total.is_some()
                && total == previous[4]
                && components[..4]
                    .iter()
                    .zip(&previous[..4])
                    .any(|(now, before)| {
                        now.zip(*before).is_some_and(|(now, before)| now != before)
                    });
            same_total_changed_breakdown
                || components
                    .iter()
                    .zip(previous)
                    .any(|(now, before)| now.zip(before).is_some_and(|(now, before)| now < before))
        });
        if input
            .zip(cached)
            .is_some_and(|(input, cached)| cached > input)
            || output
                .zip(reasoning)
                .is_some_and(|(output, reasoning)| reasoning > output)
            || agrees == Some(false)
            || contradicts_previous
        {
            self.diagnostics.inconsistent_records += 1;
        }
        self.totals = Some(TokenCounts {
            input: normalized_input,
            output,
            cache_write: Some(0),
            cache_read: cached,
        });
        self.cumulative = total;
        self.reasoning = reasoning;
        self.agrees = agrees;
        // Missing evidence cannot reset the last comparable cumulative value.
        let previous = self.previous_usage.get_or_insert([None; 5]);
        for (previous, now) in previous.iter_mut().zip(components) {
            if now.is_some() {
                *previous = now;
            }
        }
        Ok(())
    }
}
