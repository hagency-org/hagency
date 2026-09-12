//! Bounded, newest-first transcript reader, and the cached fleet meter over
//! it. This is the one metering module that touches a filesystem, and its
//! whole contract is: bounded, newest first, and every bound that bites is
//! reported, never absorbed.
//!
//! Codex files transcripts by date with no cwd in the path, so answering
//! "what did this agent use" means opening candidates until one matches, and
//! a developer machine holds hundreds of sessions with single transcripts
//! running to megabytes. An unbounded scan would make a usage request cost
//! seconds and grow with history. So: a modification-time window, a
//! file-count ceiling, a byte ceiling per file, and a traversal ceiling on
//! the walk itself. A truncated scan understates consumption, and an
//! understated number presented as a total is the failure this module exists
//! to avoid — every bound that bites is reported.
//!
//! Directories, file names and mtimes are private untrusted layout hints,
//! never authenticated Agent, project or task identity. The clock is always
//! injected; environment overrides are parsed by the caller via
//! [`ReaderLimits::from_env`], never read inside a scan.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::TokenCounts;
use crate::attribution::{
    AgentRow, FleetSummary, ScanBounds, SessionText, TranscriptSearch, agent_transcript_workspace,
    meter_agent, summarize_fleet, transcript_search,
};

/// Default window: 30 days.
pub const WINDOW_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// Default candidate ceiling per scan.
pub const MAX_FILES: u64 = 200;
/// Default bytes read per transcript. A truncated read understates, so the
/// truncation is reported rather than absorbed.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Default traversal ceiling: directory entries one scan may walk.
pub const MAX_ENTRIES: u64 = 20_000;
/// How long a computed figure stays fresh.
pub const CACHE_TTL_MS: u64 = 60_000;

/// Every bound the walk and read phases track. Keys mirror the JavaScript
/// `bounds`; the fleet scan sums by these keys, so a bound absent here is
/// silently never aggregated.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub files_seen: u64,
    pub files_read: u64,
    pub dropped_by_count: u64,
    pub dropped_by_age: u64,
    pub truncated: u64,
    /// Age-drops from a search that could NOT be narrowed to one workspace:
    /// the dropped file's cwd was never read, so it cannot be claimed as
    /// this agent's understatement.
    pub unread_outside_window: u64,
    pub entries_walked: u64,
    pub entries_unwalked: u64,
}

impl Bounds {
    fn add(&mut self, other: &Bounds) {
        self.files_seen += other.files_seen;
        self.files_read += other.files_read;
        self.dropped_by_count += other.dropped_by_count;
        self.dropped_by_age += other.dropped_by_age;
        self.truncated += other.truncated;
        self.unread_outside_window += other.unread_outside_window;
        self.entries_walked += other.entries_walked;
        self.entries_unwalked += other.entries_unwalked;
    }

    fn scan_bounds(&self) -> ScanBounds {
        ScanBounds {
            dropped_by_count: self.dropped_by_count,
            entries_unwalked: self.entries_unwalked,
        }
    }
}

/// Whether any bound actually bit, and which. `None` when the scan was
/// complete. Two claims kept apart: a bound that dropped a transcript
/// belonging to the measured workspace makes the figure an understatement; a
/// bound that dropped a candidate whose workspace was never read supports no
/// such claim and is reported separately.
#[must_use]
pub fn bounds_report(bounds: &Bounds) -> Option<String> {
    let mut understates: Vec<String> = Vec::new();
    if bounds.dropped_by_age > 0 {
        understates.push(format!(
            "{} transcript(s) older than the window",
            bounds.dropped_by_age
        ));
    }
    if bounds.dropped_by_count > 0 {
        understates.push(format!(
            "{} transcript(s) beyond the file limit",
            bounds.dropped_by_count
        ));
    }
    if bounds.truncated > 0 {
        understates.push(format!(
            "{} transcript(s) truncated at the byte limit",
            bounds.truncated
        ));
    }
    if bounds.entries_unwalked > 0 {
        understates.push(format!(
            "{} directory entr(ies) beyond the traversal limit",
            bounds.entries_unwalked
        ));
    }

    let mut unknown: Vec<String> = Vec::new();
    if bounds.unread_outside_window > 0 {
        unknown.push(format!(
            "{} candidate transcript(s) fell outside the window in a search that could not be \
             narrowed to one workspace, so whether any belong to this agent was never read",
            bounds.unread_outside_window
        ));
    }

    if understates.is_empty() && unknown.is_empty() {
        return None;
    }
    if understates.is_empty() {
        return Some(format!("scan was bounded: {}", unknown.join("; ")));
    }
    let head = format!(
        "scan was bounded and this figure understates consumption: {}",
        understates.join("; ")
    );
    if unknown.is_empty() {
        Some(head)
    } else {
        Some(format!("{head}. Separately, {}", unknown.join("; ")))
    }
}

/// Limits for one scan, with the JavaScript defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderLimits {
    pub window_ms: u64,
    pub max_files: u64,
    pub max_bytes: u64,
    pub max_entries: u64,
}

impl Default for ReaderLimits {
    fn default() -> Self {
        Self {
            window_ms: WINDOW_MS,
            max_files: MAX_FILES,
            max_bytes: MAX_BYTES,
            max_entries: MAX_ENTRIES,
        }
    }
}

impl ReaderLimits {
    /// Parse the `HAGENCY_METERING_*` overrides with the JavaScript rule:
    /// a positive base-10 integer wins, anything else (empty, zero,
    /// negative, fractional, trailing garbage) keeps the default. Values are
    /// explicit strings so the caller reads the process environment; the
    /// module itself never consults it.
    #[must_use]
    pub fn from_env(
        window_ms: Option<&str>,
        max_files: Option<&str>,
        max_bytes: Option<&str>,
        max_entries: Option<&str>,
    ) -> Self {
        Self {
            window_ms: positive_or(window_ms, WINDOW_MS),
            max_files: positive_or(max_files, MAX_FILES),
            max_bytes: positive_or(max_bytes, MAX_BYTES),
            max_entries: positive_or(max_entries, MAX_ENTRIES),
        }
    }
}

fn positive_or(value: Option<&str>, default: u64) -> u64 {
    value
        .and_then(parse_int_prefix)
        .and_then(|parsed| u64::try_from(parsed).ok())
        .filter(|parsed| *parsed > 0)
        .unwrap_or(default)
}

/// JavaScript `parseInt(v, 10)`: leading whitespace skipped, optional sign,
/// leading decimal digits taken and the rest ignored; no digits is NaN.
/// The sign is preserved so a negative cannot satisfy the positive rule.
fn parse_int_prefix(text: &str) -> Option<i128> {
    let trimmed = text.trim_start();
    let (negative, digits_source) = match trimmed.strip_prefix('+') {
        Some(rest) => (false, rest),
        None => match trimmed.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        },
    };
    let digits: String = digits_source
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return None;
    }
    let magnitude: i128 = digits.parse().ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

/// A reader bound to limits and an injected clock. The clock is a
/// millisecond timestamp, matching the JavaScript `now()` shape.
pub struct SessionReader<'a> {
    limits: ReaderLimits,
    now_ms: &'a dyn Fn() -> u64,
}

impl<'a> SessionReader<'a> {
    #[must_use]
    pub fn new(limits: ReaderLimits, now_ms: &'a dyn Fn() -> u64) -> Self {
        Self { limits, now_ms }
    }

    /// Read transcripts for one search, newest first, within the bounds.
    /// Newest first so that when a bound bites, what is dropped is the
    /// oldest and least relevant rather than an arbitrary slice.
    pub fn read(&self, search: &TranscriptSearch) -> (Vec<SessionText>, Bounds) {
        let mut bounds = Bounds::default();
        if search.dir.is_empty() || !Path::new(&search.dir).exists() {
            return (Vec::new(), bounds);
        }
        let candidates = self.list_candidates(&search.dir, search.recursive, &mut bounds);

        let cutoff = (self.now_ms)() - self.limits.window_ms;
        let mut stamped = Vec::new();
        for file in candidates {
            let Ok(metadata) = fs::metadata(&file) else {
                continue;
            };
            bounds.files_seen += 1;
            let Ok(mtime) = metadata.modified() else {
                continue;
            };
            let mtime_ms = unix_ms(mtime);
            if mtime_ms < cutoff {
                if search.narrowed {
                    bounds.dropped_by_age += 1;
                } else {
                    bounds.unread_outside_window += 1;
                }
                continue;
            }
            stamped.push((file, mtime_ms, metadata.len()));
        }
        // Newest first; equal mtimes keep listing order, like the stable
        // JavaScript sort.
        stamped.sort_by_key(|entry| std::cmp::Reverse(entry.1));

        let mut out = Vec::new();
        for (file, _mtime, _size) in stamped {
            if bounds.files_read >= self.limits.max_files {
                bounds.dropped_by_count += 1;
                continue;
            }
            let Ok(bytes) = fs::read(&file) else {
                continue;
            };
            // Cut at a line boundary: a half-line is unparseable and would
            // be skipped silently, which looks the same as a transcript with
            // fewer records. `lastIndexOf('\n', maxBytes)` searches from
            // index `maxBytes` inclusive, and a miss returns -1, whose slice
            // drops only the final character — replicated exactly.
            let text = if bytes.len() > self.limits.max_bytes as usize {
                let limit = self.limits.max_bytes as usize;
                let window_end = bytes.len().min(limit.saturating_add(1));
                let cut = bytes[..window_end]
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .unwrap_or(bytes.len() - 1);
                bounds.truncated += 1;
                String::from_utf8_lossy(&bytes[..cut]).into_owned()
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            };
            bounds.files_read += 1;
            out.push(SessionText {
                file: file.display().to_string(),
                text,
            });
        }
        (out, bounds)
    }

    /// List `.jsonl` candidates under a root, breadth-first, descending only
    /// when `recursive`. The walk is bounded by `max_entries`; entries past
    /// the ceiling are counted, not silently skipped. Unreadable
    /// directories are skipped whole, like the JavaScript `try/catch`.
    fn list_candidates(&self, root: &str, recursive: bool, bounds: &mut Bounds) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(PathBuf::from(root));
        while let Some(dir) = queue.pop_front() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if bounds.entries_walked >= self.limits.max_entries {
                    bounds.entries_unwalked += 1;
                    continue;
                }
                bounds.entries_walked += 1;
                let full = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
                if is_dir {
                    if recursive {
                        queue.push_back(full);
                    }
                } else if name.ends_with(".jsonl") {
                    // JavaScript `endsWith('.jsonl')`: a bare `.jsonl` file
                    // matches (Rust's `Path::extension` would not).
                    out.push(full);
                }
            }
        }
        out
    }
}

fn unix_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The value `meter_fleet` returns: the fleet summary fields flattened over
/// by the JavaScript spread (`{...summary, scanned, boundsReason, cached,
/// computedAt}`). `bounds_reason` is about a scan that stopped early;
/// `reason` is about agents that could not be attributed at all — two
/// separate caveats, never merged.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetMetering {
    pub agents: Vec<AgentRow>,
    pub totals: Option<TokenCounts>,
    pub total: Option<u64>,
    pub attributed: usize,
    pub unattributed: usize,
    pub reason: Option<String>,
    pub scanned: Bounds,
    pub bounds_reason: Option<String>,
    pub cached: bool,
    pub computed_at: u64,
}

/// Time-based fleet cache keyed over the fleet's identity, not just the
/// clock: which agents exist, their framework, and the workspace their
/// transcripts are under. A heartbeat that changes none of those still hits
/// the cache, which is the point; a changed fleet does not.
pub struct FleetCache {
    at: u64,
    key: Option<String>,
    value: Option<FleetMetering>,
}

impl Default for FleetCache {
    fn default() -> Self {
        Self::new()
    }
}

impl FleetCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            at: 0,
            key: None,
            value: None,
        }
    }

    /// Drop the cache. For tests, and for a caller that has just changed the
    /// fleet or its home directory.
    pub fn reset(&mut self) {
        self.at = 0;
        self.key = None;
        self.value = None;
    }

    fn get(&self, key: &str, now_ms: u64, ttl_ms: u64) -> Option<FleetMetering> {
        if let Some(value) = &self.value
            && self.key.as_deref() == Some(key)
            && now_ms.saturating_sub(self.at) < ttl_ms
        {
            let mut stamped = value.clone();
            stamped.cached = true;
            stamped.computed_at = self.at;
            return Some(stamped);
        }
        None
    }
}

/// Meter a fleet, cached. `agents` are serialized agent records — they carry
/// `type` and `workspacePath`. The home directory is part of the cache key
/// because it changes every search descriptor.
// The JavaScript `meterFleet` carries exactly these inputs; grouping them
// behind a builder would hide the injected clock and cache the port pins.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn meter_fleet(
    agents: &[Value],
    home_dir: &str,
    process_cwd: &str,
    limits: ReaderLimits,
    cache_ttl_ms: u64,
    now_ms: u64,
    force: bool,
    cache: &mut FleetCache,
) -> FleetMetering {
    let key = fleet_key(agents, home_dir);
    if !force && let Some(hit) = cache.get(&key, now_ms, cache_ttl_ms) {
        return hit;
    }

    let mut rows: Vec<AgentRow> = Vec::new();
    let mut scanned = Bounds::default();
    for agent in agents {
        let workspace = agent_transcript_workspace(agent);
        let framework = agent
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let search = workspace
            .as_deref()
            .and_then(|workspace| transcript_search(&framework, workspace, home_dir));
        let (sessions, bounds) = match &search {
            Some(search) => SessionReader::new(limits, &|| now_ms).read(search),
            None => (Vec::new(), Bounds::default()),
        };
        // Attach the scan caveat to each row the way the JavaScript does:
        // the fleet-level `boundsReason` names the whole scan; the row keeps
        // its own attribution reason.
        let row = meter_agent(
            agent,
            home_dir,
            process_cwd,
            &sessions,
            Some(bounds.scan_bounds()),
        );
        scanned.add(&bounds);
        rows.push(row);
    }

    let FleetSummary {
        agents,
        totals,
        total,
        attributed,
        unattributed,
        reason,
    } = summarize_fleet(rows, process_cwd);
    let bounds_reason = bounds_report(&scanned);
    let value = FleetMetering {
        agents,
        totals,
        total,
        attributed,
        unattributed,
        reason,
        scanned,
        bounds_reason,
        cached: false,
        computed_at: now_ms,
    };
    cache.at = now_ms;
    cache.key = Some(key);
    cache.value = Some(value.clone());
    value
}

fn fleet_key(agents: &[Value], home_dir: &str) -> String {
    let mut triples: Vec<[String; 3]> = agents
        .iter()
        .map(|agent| {
            [
                agent
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                agent
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                agent_transcript_workspace(agent).unwrap_or_default(),
            ]
        })
        .collect();
    triples.sort_by(|a, b| a[0].cmp(&b[0]));
    format!(
        "{home_dir}|{}",
        serde_json::to_string(&triples).unwrap_or_default()
    )
}
