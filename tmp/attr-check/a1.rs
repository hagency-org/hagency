//! Transcript attribution search: locating an agent's session transcripts and
//! totalling them from caller-injected session text.
//!
//! The chain is agent → workspace → session transcripts → tokens, and every
//! link is verified rather than assumed: a wrong link attributes one agent's
//! consumption to another, which is worse than reporting none. Search
//! descriptors (`dir`, `narrowed`, `recursive`) are private, untrusted hints
//! about a CLI's on-disk layout. A directory name or a recorded `cwd` never
//! establishes authenticated Agent, project or task identity.
//!
//! This module performs no filesystem access. Session text is injected by the
//! caller, as is the working directory that JavaScript's `path.resolve` would
//! take from the process. Every case the JavaScript cannot answer returns
//! `available: false` with a reason; a refused or partial measurement is
//! never replaced by a zero.
//!
//! Documented divergences from the JavaScript oracle (all deliberate
//! corrections in the ADR-055 direction, recorded in ADR-118):
//!
//! - A session the native parser refuses (duplicate JSON keys, a repeated
//!   message identity with conflicting usage, unsafe counters, capacity) is
//!   counted in `skipped`; the JavaScript coerces or silently keeps the first
//!   duplicate and reports a number.
//! - A transcript whose own `cwd` cannot be established (including a native
//!   observation of conflicting workspace hints, which JavaScript resolves by
//!   first-wins) is never attributed.
//! - Missing usage stays unknown (`null`), including in sums; the JavaScript
//!   oracle coerces missing fields to `0`. Vectors shared with the oracle
//!   avoid these inputs; native-only tests pin the corrected behavior.
//! - Non-string agent fields (`type` an object, `name` a number) are treated
//!   as absent rather than stringified; serialized agent records never carry
//!   those shapes.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::{Framework, TokenCounts, parse_session};

const OCTOS_REASON: &str = "octos session files record no usage object and no cwd, so consumption can be \
 neither counted nor attributed from them";
const NO_LOCATION_VERIFIED: &str = "no transcript location verified for this adapter";
const NO_ADAPTER: &str = "no metering adapter exists for this framework";
const NO_WORKSPACE: &str = "no workspace recorded for this agent, so its transcripts cannot be located; \
 a running agent reports one and a stopped one may never have";
const NO_TRANSCRIPTS_YET: &str = "no transcripts found for this workspace yet";

/// Where to look for one framework's transcripts, given a workspace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSearch {
    /// Private layout hint. Never a filesystem capability.
    pub dir: String,
    /// True when the directory itself implies the workspace, so an
    /// out-of-window transcript there is still this agent's older spend.
    pub narrowed: bool,
    /// True when the transcripts sit below the directory, not directly in it.
    pub recursive: bool,
}

/// What an injected scan could not reach. Mirrors `readSessions.bounds`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanBounds {
    pub dropped_by_count: u64,
    pub entries_unwalked: u64,
}

/// One already-read candidate transcript. No file is opened here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionText {
    pub file: String,
    pub text: String,
}

/// Per-file totals for an attributed session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileTotals {
    pub file: String,
    pub totals: TokenCounts,
}

/// One agent's metering row. Unavailable rows carry a reason and never a
/// total; available rows carry totals, where `null` means unknown, not zero.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRow {
    pub agent: Option<String>,
    pub available: bool,
    pub framework: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totals: Option<TokenCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileTotals>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Fleet summary over [`AgentRow`] values, keeping every gap visible.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetSummary {
    pub agents: Vec<AgentRow>,
    pub totals: Option<TokenCounts>,
    pub total: Option<u64>,
    pub attributed: usize,
    pub unattributed: usize,
    pub reason: Option<String>,
}

/// Claude's directory name for a working directory: forward only, every `/`
/// replaced by `-`. Non-absolute paths have no directory.
#[must_use]
pub fn claude_project_dir(workspace_path: &str) -> Option<String> {
    if !workspace_path.starts_with('/') {
        return None;
    }
    Some(workspace_path.replace('/', "-"))
}

/// Where to look for one framework's transcripts, given a workspace.
/// Framework matching is case-insensitive, as in the JavaScript; unknown
/// frameworks and relative Claude workspaces have no search.
#[must_use]
pub fn transcript_search(
    framework: &str,
    workspace_path: &str,
    home_dir: &str,
) -> Option<TranscriptSearch> {
    match framework.to_lowercase().as_str() {
        "claude" => {
            // One directory per workspace, transcripts directly inside it.
            let dir = claude_project_dir(workspace_path)?;
            Some(TranscriptSearch {
                dir: posix_join(&[home_dir, ".claude", "projects", &dir]),
                narrowed: true,
                recursive: false,
            })
        }
        // Filed by date under a nested `YYYY/MM/DD` tree, so nothing can be
        // narrowed by path and the walk has to descend.
        "codex" => Some(TranscriptSearch {
            dir: posix_join(&[home_dir, ".codex", "sessions"]),
            narrowed: false,
            recursive: true,
        }),
        _ => None,
    }
}

/// The workspace whose transcripts belong to an agent: `lastWorkspacePath`
/// first (consumption outlives the process), then `workspacePath`, and
/// `workdir`/`homeDir` only for an on-demand runner. Values are trimmed as
/// JavaScript trims them; the first non-empty string wins, else `None`.
#[must_use]
pub fn agent_transcript_workspace(agent: &Value) -> Option<String> {
    let on_demand = agent
        .get("runner")
        .and_then(|runner| string_field(runner, "mode"))
        .is_some_and(|mode| mode == "on-demand");
    let mut all: Vec<&str> = [
        string_field(agent, "lastWorkspacePath"),
        string_field(agent, "workspacePath"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if on_demand {
        all.extend(string_field(agent, "workdir"));
        all.extend(string_field(agent, "homeDir"));
    }
    all.iter()
        .map(|value| js_trim(value))
        .find(|trimmed| !trimmed.is_empty())
        .map(str::to_owned)
}

/// Total one agent's consumption from injected `(file, text)` sessions. Pure:
/// no filesystem access, and `process_cwd` stands in for the working
/// directory JavaScript's `path.resolve` would read from the process. Every
/// case that cannot be answered returns `available: false` with the
/// JavaScript reason string; unknown values stay `null`, never zero.
#[must_use]
pub fn meter_agent(
    agent: &Value,
    home_dir: &str,
    process_cwd: &str,
    sessions: &[SessionText],
    bounds: Option<ScanBounds>,
) -> AgentRow {
    let agent_name = agent.get("name").and_then(Value::as_str).map(str::to_owned);
    let framework = js_string(agent.get("type")).to_lowercase();
    if let Some(reason) = unsupported_reason(&framework) {
        return unavailable(agent_name, framework, reason.to_owned());
    }

    let Some(workspace) = agent_transcript_workspace(agent) else {
        return unavailable(agent_name, framework, NO_WORKSPACE.to_owned());
    };

    if transcript_search(&framework, &workspace, home_dir).is_none() {
        let reason = format!("no transcript location known for {framework}");
        return unavailable(agent_name, framework, reason);
    }

    let parser_framework = match framework.as_str() {
        "claude" => Framework::Claude,
        _ => Framework::Codex,
    };
    let mut totals = zero_counts();
    let mut files = Vec::new();
    let mut matched: u32 = 0;
    let mut skipped: u32 = 0;

    for session in sessions {
        // The transcript's own cwd decides, even when the directory already
        // implied it: the directory mapping is ambiguous backwards, and a
        // transcript moved between directories would otherwise be counted
        // against the wrong agent. A parse the native parser refuses
        // (duplicate keys, conflicting identities, unsafe counters) is
        // unattributable evidence and is counted as skipped, never coerced.
        let parsed = match parse_session(parser_framework, &session.text) {
            Ok(report) => report,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let recorded = parsed.workspace_hint.as_deref().is_some_and(|cwd| {
            resolve_against(process_cwd, cwd) == resolve_against(process_cwd, &workspace)
        });
        if !recorded {
            skipped += 1;
            continue;
        }
        let session_totals = parsed.totals.unwrap_or_else(unknown_counts);
        totals = add_counts(totals, &session_totals);
        files.push(FileTotals {
            file: session.file.clone(),
            totals: session_totals,
        });
        matched += 1;
    }

    if matched == 0 {
        // A zero-match reason must not imply the agent did no work: the
        // branches are additive, so a scan that both opened other
        // workspaces' transcripts and stopped at its bounds states both.
        let unreached = bounds.map_or(0, |b| b.dropped_by_count + b.entries_unwalked);
        let mut parts: Vec<String> = Vec::new();
        if skipped > 0 {
            parts.push(format!(
                "opened {skipped} transcript(s), none of which recorded this workspace"
            ));
        }
        if unreached > 0 {
            parts.push(format!(
                "{unreached} further candidate transcript(s) were never opened because the scan \
                 stopped at its bounds, so this is not evidence the agent did no work — raise \
                 HAGENCY_METERING_MAX_FILES to widen the scan"
            ));
        }
        let reason = if parts.is_empty() {
            NO_TRANSCRIPTS_YET.to_owned()
        } else {
            parts.join("; and ")
        };
        return AgentRow {
            agent: agent_name,
            available: false,
            framework,
            workspace: Some(workspace),
            totals: None,
            total: None,
            sessions: None,
            skipped: None,
            files: None,
            reason: Some(reason),
        };
    }

    AgentRow {
        agent: agent_name,
        available: true,
        framework,
        workspace: Some(workspace),
        totals: Some(totals),
        total: sum_options(&[
            totals.input,
            totals.output,
            totals.cache_write,
            totals.cache_read,
        ]),
        sessions: Some(matched),
        skipped: Some(skipped),
        files: Some(files),
        reason: None,
    }
}

/// Total a fleet, keeping every gap visible. Agents sharing a workspace are
/// reported as ambiguous instead of summed: transcripts record the directory,
/// not which agent hagency started there.
#[must_use]
pub fn summarize_fleet(rows: Vec<AgentRow>, process_cwd: &str) -> FleetSummary {
    let mut members: HashMap<String, Vec<Option<String>>> = HashMap::new();
    for row in &rows {
        let Some(key) = group_key(row, process_cwd) else {
            continue;
        };
        members.entry(key).or_default().push(row.agent.clone());
    }

    let agents = rows
        .into_iter()
        .map(|mut row| {
            let Some(key) = group_key(&row, process_cwd) else {
                return row;
            };
            let shared = &members[&key];
            if shared.len() > 1 {
                let others = shared
                    .iter()
                    .filter(|agent| **agent != row.agent)
                    .map(|agent| agent.clone().unwrap_or_else(|| "null".to_owned()))
                    .collect::<Vec<_>>()
                    .join(", ");
                row.available = false;
                row.totals = None;
                row.total = None;
                row.reason = Some(format!(
                    "workspace is shared with {others}; transcripts record the directory, not \
                     which agent ran there, so consumption cannot be attributed"
                ));
            }
            row
        })
        .collect::<Vec<_>>();

    let mut totals = zero_counts();
    let mut attributed = 0;
    for row in &agents {
        if row.available {
            attributed += 1;
            totals = match row.totals {
                Some(row_totals) => add_counts(totals, &row_totals),
                None => unknown_counts(),
            };
        }
    }
    let unattributed = agents.len() - attributed;
    let reason = (unattributed > 0).then(|| {
        format!(
            "{unattributed} of {} agents could not be attributed",
            agents.len()
        )
    });
    FleetSummary {
        totals: (attributed > 0).then_some(totals),
        total: if attributed > 0 {
            sum_options(&[
                totals.input,
                totals.output,
                totals.cache_write,
                totals.cache_read,
            ])
        } else {
            None
        },
        agents,
        attributed,
        unattributed,
        reason,
    }
}

/// The JavaScript `meteringSupport` reason for a framework without a parser,
/// or `None` when the framework is meterable.
fn unsupported_reason(framework: &str) -> Option<&'static str> {
    match framework {
        "claude" | "codex" => None,
        "octos" => Some(OCTOS_REASON),
        "codex-acp" | "hermes" => Some(NO_LOCATION_VERIFIED),
        _ => Some(NO_ADAPTER),
    }
}

fn unavailable(agent: Option<String>, framework: String, reason: String) -> AgentRow {
    AgentRow {
        agent,
        available: false,
        framework,
        workspace: None,
        totals: None,
        total: None,
        sessions: None,
        skipped: None,
        files: None,
        reason: Some(reason),
    }
}

fn group_key(row: &AgentRow, process_cwd: &str) -> Option<String> {
    row.workspace
        .as_deref()
        .filter(|workspace| !workspace.is_empty())
        .map(|workspace| resolve_against(process_cwd, workspace))
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    // JavaScript keeps only strings; every other shape reads as empty.
    match value.get(key) {
        Some(Value::String(text)) => Some(text.as_str()),
        _ => None,
    }
}

/// `String(v ?? '')` for the shapes an agent record can carry. Objects and
/// arrays never name a framework in serialized agent records and read as
/// empty here.
fn js_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(_) => String::new(),
    }
}

/// JavaScript `String.prototype.trim`: White_Space plus U+FEFF. Rust's
/// `char::is_whitespace` excludes U+FEFF, so the set is spelled out.
fn js_trim(value: &str) -> &str {
    value.trim_matches(|c: char| {
        matches!(
            c,
            '\u{9}'..='\u{d}'
                | '\u{20}'
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
        )
    })
}

/// `path.join` for POSIX: drop empty parts, join with `/`, then normalize
/// `.` and `..` away. Trailing-slash preservation never applies here because
/// the final segment is always a non-empty literal.
fn posix_join(parts: &[&str]) -> String {
    let joined = parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/");
    posix_normalize(&joined)
}

fn posix_normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| *last != "..") {
                    segments.pop();
                } else if !absolute {
                    segments.push("..");
                }
            }
            other => segments.push(other),
        }
    }
    let joined = segments.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// `path.resolve` against an explicit working directory, because the
/// JavaScript resolves relative paths against the process cwd.
fn resolve_against(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        posix_normalize(path)
    } else {
        posix_normalize(&format!("{base}/{path}"))
    }
}

fn zero_counts() -> TokenCounts {
    TokenCounts {
        input: Some(0),
        output: Some(0),
        cache_write: Some(0),
        cache_read: Some(0),
    }
}

fn unknown_counts() -> TokenCounts {
    TokenCounts {
        input: None,
        output: None,
        cache_write: None,
        cache_read: None,
    }
}

fn opt_add(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    a?.checked_add(b?)
}

/// Add known values; an unknown or overflowing kind stays unknown rather
/// than being coerced to a number.
fn add_counts(a: TokenCounts, b: &TokenCounts) -> TokenCounts {
    TokenCounts {
        input: opt_add(a.input, b.input),
        output: opt_add(a.output, b.output),
        cache_write: opt_add(a.cache_write, b.cache_write),
        cache_read: opt_add(a.cache_read, b.cache_read),
    }
}

fn sum_options(values: &[Option<u64>]) -> Option<u64> {
    values
        .iter()
        .try_fold(0u64, |total, value| total.checked_add((*value)?))
}
