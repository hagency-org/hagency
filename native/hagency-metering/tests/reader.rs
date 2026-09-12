//! Oracle replay and native-pinned tests for the bounded transcript reader.
//!
//! The fixture is produced by `native/scripts/reader-vectors.mjs`, which
//! builds deterministic synthetic transcript trees under a fresh temporary
//! directory and runs the retained JavaScript reader (`lib/metering/reader.js`)
//! with an injected clock. Replay rebuilds each recorded tree with the same
//! mtimes and compares files, bounds, reports and fleet values exactly.
//!
//! Trees are built under the workspace `tmp/` directory (git-ignored) because
//! the sandbox denies `/tmp`; `hagency-metering` deliberately has no
//! `tempfile` dev-dependency, so directories are made with `std::fs` under a
//! unique name and removed afterwards.

use std::fs;
use std::ops::Deref;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use hagency_metering::attribution::transcript_search;
use hagency_metering::reader::{
    Bounds, FleetCache, ReaderLimits, SessionReader, bounds_report, meter_fleet,
};
use serde::Deserialize;
use serde_json::Value;

const HOME_TOKEN: &str = "<HOME>";
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../fixtures/reader.json")).unwrap()
}

/// A fresh throwaway home under the workspace tmp dir, removed on drop.
struct TempHome(PathBuf);

impl Deref for TempHome {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        // Best effort; a stray tree under tmp/ is inert.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp_home(label: &str) -> TempHome {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("tmp");
    let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = root.join(format!(
        "reader-test-{}-{label}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("create temp home");
    TempHome(path)
}

fn set_mtime(path: &Path, offset_ms: i64) {
    // Recorded mtimes are `now + offset` with now > 0, so the absolute value
    // is the timestamp; a negative total is clamped to the epoch.
    let when = UNIX_EPOCH + Duration::from_millis(offset_ms.unsigned_abs());
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for mtime");
    file.set_modified(when).expect("set mtime");
}

/// Build the recorded relative tree under `home` with the recorded mtimes.
fn build_tree(home: &Path, tree: &[Value], now_ms: i64) {
    for node in tree {
        let relative = node["path"].as_str().expect("tree path");
        let content = node["content"].as_str().expect("tree content");
        let offset = node["mtimeOffsetMs"].as_i64().unwrap_or(0);
        let full = home.join(relative);
        fs::create_dir_all(full.parent().expect("parent")).expect("create dirs");
        fs::write(&full, content).expect("write tree file");
        set_mtime(&full, now_ms + offset);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LimitsSpec {
    #[serde(default)]
    window_ms: Option<u64>,
    #[serde(default)]
    max_files: Option<u64>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_entries: Option<u64>,
}

impl LimitsSpec {
    fn resolve(&self) -> ReaderLimits {
        let defaults = ReaderLimits::default();
        ReaderLimits {
            window_ms: self.window_ms.unwrap_or(defaults.window_ms),
            max_files: self.max_files.unwrap_or(defaults.max_files),
            max_bytes: self.max_bytes.unwrap_or(defaults.max_bytes),
            max_entries: self.max_entries.unwrap_or(defaults.max_entries),
        }
    }
}

/// `<HOME>/…` → `<actual home>/…` for comparison.
fn actualize(text: &str, home: &Path) -> String {
    text.replace(&format!("{HOME_TOKEN}/"), &format!("{}/", home.display()))
}

fn relativize(text: &str, home: &Path) -> String {
    let text = text.replace(&format!("{}/", home.display()), &format!("{HOME_TOKEN}/"));
    // The walk joins with the host separator; the recorded oracle uses `/`.
    // Only the components below the home token are rewritten, so the actual
    // home spelling is never compared.
    if cfg!(windows) && text.starts_with(HOME_TOKEN) {
        text.replace('\\', "/")
    } else {
        text
    }
}

#[test]
fn native_metering_reader_vectors() {
    let fixture = fixture();
    let now = fixture["now"].as_i64().expect("recorded clock");
    let vectors = &fixture["vectors"];

    for vector in vectors["reads"].as_array().expect("read vectors") {
        let name = vector["name"].as_str().expect("vector name");
        let home = temp_home(name);
        build_tree(&home, vector["tree"].as_array().expect("tree"), now);

        // The unreadable-directory case is portable only where a permission
        // mask actually denies the walk; when record and replay disagree on
        // denial, the environment differs and comparison is skipped.
        if let Some(recorded_denied) = vector["denied"].as_bool() {
            // A POSIX permission mask is the only portable way to deny a
            // directory walk; Windows ACLs are not modelled here, so the
            // vector is skipped rather than compared against a walk that
            // was never denied.
            #[cfg(not(unix))]
            {
                let _ = recorded_denied;
                eprintln!("skipping {name}: no permission mask denies a directory walk here");
                continue;
            }
            #[cfg(unix)]
            {
                let denied_dir = home.join(".codex/sessions/2026/09/01/锁");
                let _ = fs::set_permissions(&denied_dir, fs::Permissions::from_mode(0o000));
                let denied = fs::read_dir(&denied_dir).is_err();
                let _ = fs::set_permissions(&denied_dir, fs::Permissions::from_mode(0o755));
                if denied != recorded_denied {
                    eprintln!("skipping {name}: directory denial differs in this environment");
                    continue;
                }
                // Rebuild the mask for the actual read below.
                let _ = fs::set_permissions(&denied_dir, fs::Permissions::from_mode(0o000));
                let result = run_read(vector, &home, now);
                let _ = fs::set_permissions(&denied_dir, fs::Permissions::from_mode(0o755));
                assert_read_matches(vector, &result, &home, name);
                continue;
            }
        }

        let result = run_read(vector, &home, now);
        assert_read_matches(vector, &result, &home, name);
    }

    for vector in vectors["reports"].as_array().expect("report vectors") {
        let bounds: Bounds =
            serde_json::from_value(vector["bounds"].clone()).expect("bounds decode");
        let expected = match &vector["expected"] {
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            other => panic!("unexpected report shape: {other}"),
        };
        assert_eq!(
            bounds_report(&bounds),
            expected,
            "report vector {}",
            vector["name"]
        );
    }

    for vector in vectors["fleets"].as_array().expect("fleet vectors") {
        let name = vector["name"].as_str().expect("fleet vector name");
        let home = temp_home(name);
        build_tree(&home, vector["tree"].as_array().expect("tree"), now);
        let mut cache = FleetCache::new();
        for (index, step) in vector["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .enumerate()
        {
            let agents = step["agents"].as_array().expect("agents").clone();
            let now_step = step["now"].as_u64().expect("step clock");
            let force = step["force"].as_bool().unwrap_or(false);
            let value = meter_fleet(
                &agents,
                &home.display().to_string(),
                "/",
                ReaderLimits::default(),
                60_000,
                now_step,
                force,
                &mut cache,
            );
            let mut projected = serde_json::to_value(&value).unwrap();
            for agent in projected["agents"].as_array_mut().expect("agents") {
                if let Some(Value::Array(files)) = agent.get_mut("files") {
                    for file in files {
                        if let Some(text) = file["file"].as_str() {
                            file["file"] = Value::String(relativize(text, &home));
                        }
                    }
                }
            }
            assert_eq!(
                projected, step["expected"],
                "fleet vector {name} step {index}"
            );
        }
    }
}

fn run_read(vector: &Value, home: &Path, now: i64) -> (Vec<(String, String)>, Bounds) {
    let framework = vector["framework"].as_str().expect("framework");
    let workspace = vector["workspacePath"].as_str().expect("workspace");
    let limits: LimitsSpec =
        serde_json::from_value(vector["limits"].clone()).expect("limits decode");
    let search = transcript_search(framework, workspace, &home.display().to_string())
        .expect("search descriptor");
    let clock = now as u64;
    let tick = || clock;
    let reader = SessionReader::new(limits.resolve(), &tick);
    let (sessions, bounds) = reader.read(&search);
    (
        sessions
            .into_iter()
            .map(|session| (session.file, session.text))
            .collect(),
        bounds,
    )
}

fn assert_read_matches(
    vector: &Value,
    result: &(Vec<(String, String)>, Bounds),
    home: &Path,
    name: &str,
) {
    let (files, bounds) = result;
    let expected_bounds: Bounds =
        serde_json::from_value(vector["expected"]["bounds"].clone()).expect("bounds decode");
    assert_eq!(*bounds, expected_bounds, "bounds for {name}");
    let expected_files = vector["expected"]["files"].as_array().expect("files");
    assert_eq!(files.len(), expected_files.len(), "file count for {name}");
    for (actual, expected) in files.iter().zip(expected_files) {
        // Paths compare by component: the walk joins with the host separator
        // while the recorded oracle path uses `/`.
        assert_eq!(
            Path::new(&actual.0),
            Path::new(&actualize(expected["file"].as_str().unwrap(), home)),
            "file for {name}"
        );
        assert_eq!(
            actual.1,
            expected["text"].as_str().unwrap(),
            "text for {name}"
        );
    }
}

#[test]
fn native_metering_reader_bounds_are_reported() {
    let home = temp_home("bounds");
    let now = 1_800_000_000_000u64;
    // Narrowed (claude) search: an out-of-window file is this agent's own
    // older spend, so the figure understates.
    let claude_ws = "/Users/fixture/work";
    fs::create_dir_all(home.join(".claude/projects/-Users-fixture-work")).unwrap();
    for (name, age_ms, tokens) in [
        ("current.jsonl", 3_600_000u64, 3),
        ("old.jsonl", 40 * 86_400_000, 2),
    ] {
        let path = home.join(format!(".claude/projects/-Users-fixture-work/{name}"));
        let text = (0..tokens)
            .map(|i| {
                serde_json::json!({
                    "cwd": claude_ws, "uuid": format!("{name}-{i}"),
                    "message": {"usage": {"input_tokens": 10, "output_tokens": 5,
                        "cache_creation_input_tokens": 1, "cache_read_input_tokens": 2}}
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(&path, text).unwrap();
        set_mtime(&path, now as i64 - age_ms as i64);
    }
    let search = transcript_search("claude", claude_ws, &home.display().to_string()).unwrap();
    let (files, bounds) = SessionReader::new(ReaderLimits::default(), &|| now).read(&search);
    assert_eq!(files.len(), 1);
    assert_eq!(bounds.files_seen, 2);
    assert_eq!(bounds.dropped_by_age, 1);
    assert_eq!(bounds.unread_outside_window, 0);
    assert_eq!(
        bounds_report(&bounds).as_deref(),
        Some(
            "scan was bounded and this figure understates consumption: 1 transcript(s) older than the window"
        )
    );

    // Non-narrowed (codex) search: the same drop reads as unknown ownership,
    // never an understatement — and the wording says so.
    let codex_home = temp_home("bounds-codex");
    let dir = codex_home.join(".codex/sessions/2026/01/02");
    fs::create_dir_all(&dir).unwrap();
    let old = dir.join("rollout-old.jsonl");
    fs::write(&old, "{}\n").unwrap();
    set_mtime(&old, now as i64 - 40 * 86_400_000);
    let search = transcript_search("codex", "/w", &codex_home.display().to_string()).unwrap();
    let (files, bounds) = SessionReader::new(ReaderLimits::default(), &|| now).read(&search);
    assert!(files.is_empty());
    assert_eq!(bounds.unread_outside_window, 1);
    assert_eq!(
        bounds_report(&bounds).as_deref(),
        Some(
            "scan was bounded: 1 candidate transcript(s) fell outside the window in a search that could not be narrowed to one workspace, so whether any belong to this agent was never read"
        )
    );

    // A complete scan reports nothing at all.
    assert_eq!(bounds_report(&Bounds::default()), None);

    // File ceiling, truncation and traversal ceiling all bite and are named,
    // and the two claim kinds compose with ". Separately, ".
    let both = Bounds {
        dropped_by_count: 2,
        truncated: 1,
        entries_unwalked: 9,
        unread_outside_window: 4,
        ..Bounds::default()
    };
    let report = bounds_report(&both).unwrap();
    assert!(report.starts_with(
        "scan was bounded and this figure understates consumption: \
         2 transcript(s) beyond the file limit; \
         1 transcript(s) truncated at the byte limit; \
         9 directory entr(ies) beyond the traversal limit"
    ));
    assert!(report.ends_with(
        ". Separately, 4 candidate transcript(s) fell outside the window in a search that \
         could not be narrowed to one workspace, so whether any belong to this agent was never read"
    ));
}

#[test]
fn native_metering_reader_layout_discovery() {
    let now = 1_800_000_000_000u64;
    let home = temp_home("layout");
    // Codex files by date: a nested YYYY/MM/DD tree must be discovered, and
    // the flat list that missed it for years is the bug this pins.
    for (i, (date, age_ms)) in [("2026/09/01", 3u64), ("2026/09/02", 2), ("2026/08/31", 1)]
        .iter()
        .enumerate()
    {
        let dir = home.join(format!(".codex/sessions/{date}"));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("rollout-{i}.jsonl"));
        fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"/Users/fixture/work\"}}}}\n{}\n",
                serde_json::json!({
                    "payload": {"cwd": "/Users/fixture/work", "type": "token_count",
                        "info": {"total_token_usage": {"input_tokens": 900, "cached_input_tokens": 100,
                            "output_tokens": 100, "reasoning_output_tokens": 50, "total_tokens": 1000}}}
                })
            ),
        )
        .unwrap();
        set_mtime(&path, now as i64 - *age_ms as i64 * 3_600_000);
    }
    let search =
        transcript_search("codex", "/Users/fixture/work", &home.display().to_string()).unwrap();
    assert!(search.recursive);
    assert!(!search.narrowed);
    let (files, bounds) = SessionReader::new(ReaderLimits::default(), &|| now).read(&search);
    assert_eq!(files.len(), 3, "nested date tree discovered");
    assert_eq!(
        bounds.entries_walked, 9,
        "year, month, day directories and files"
    );
    assert!(
        Path::new(&files[0].file).ends_with("2026/08/31/rollout-2.jsonl"),
        "newest first"
    );
    assert!(
        Path::new(&files[2].file).ends_with("2026/09/01/rollout-0.jsonl"),
        "oldest last"
    );

    // Claude stays flat: a transcript below the project directory is not
    // found, so a sibling workspace's files are never opened on its budget.
    let claude_home = temp_home("layout-claude");
    let project = claude_home.join(".claude/projects/-Users-fixture-work");
    fs::create_dir_all(project.join("nested")).unwrap();
    let flat = project.join("session.jsonl");
    fs::write(&flat, "{}\n").unwrap();
    set_mtime(&flat, now as i64 - 60_000);
    let nested = project.join("nested/deeper.jsonl");
    fs::write(&nested, "{}\n").unwrap();
    set_mtime(&nested, now as i64 - 30_000);
    let search = transcript_search(
        "claude",
        "/Users/fixture/work",
        &claude_home.display().to_string(),
    )
    .unwrap();
    assert!(!search.recursive);
    assert!(search.narrowed);
    let (files, bounds) = SessionReader::new(ReaderLimits::default(), &|| now).read(&search);
    assert_eq!(files.len(), 1, "flat project directory only");
    assert!(Path::new(&files[0].file).ends_with("session.jsonl"));
    assert_eq!(
        bounds.entries_walked, 2,
        "session.jsonl and the nested directory"
    );
}

#[test]
fn native_metering_reader_cache() {
    let now = 1_800_000_000_000u64;
    let home = temp_home("cache");
    let dir = home.join(".claude/projects/-Users-fixture-work");
    fs::create_dir_all(&dir).unwrap();
    let session = dir.join("session.jsonl");
    fs::write(
        &session,
        format!(
            "{}\n",
            serde_json::json!({
                "cwd": "/Users/fixture/work", "uuid": "u1",
                "message": {"usage": {"input_tokens": 10, "output_tokens": 5,
                    "cache_creation_input_tokens": 1, "cache_read_input_tokens": 2}}
            })
        ),
    )
    .unwrap();
    set_mtime(&session, now as i64 - 60_000);
    let agents: Vec<Value> = vec![serde_json::json!({
        "name": "a1", "type": "claude", "workspacePath": "/Users/fixture/work"
    })];
    let changed: Vec<Value> = vec![serde_json::json!({
        "name": "a2", "type": "claude", "workspacePath": "/Users/fixture/work"
    })];
    let home_str = home.display().to_string();

    let mut cache = FleetCache::new();
    // First computation: fresh, stamped at the clock, not cached.
    let first = meter_fleet(
        &agents,
        &home_str,
        "/",
        ReaderLimits::default(),
        60_000,
        now,
        true,
        &mut cache,
    );
    assert!(!first.cached);
    assert_eq!(first.computed_at, now);
    assert_eq!(first.attributed, 1);

    // Inside the TTL the same fleet is an object lookup with the original
    // stamp — a page refresh, not a rescan.
    let hit = meter_fleet(
        &agents,
        &home_str,
        "/",
        ReaderLimits::default(),
        60_000,
        now + 30_000,
        false,
        &mut cache,
    );
    assert!(hit.cached);
    assert_eq!(hit.computed_at, now);

    // At exactly the TTL the entry is stale.
    let stale = meter_fleet(
        &agents,
        &home_str,
        "/",
        ReaderLimits::default(),
        60_000,
        now + 60_000,
        false,
        &mut cache,
    );
    assert!(!stale.cached);
    assert_eq!(stale.computed_at, now + 60_000);

    // A changed fleet never reads the old answer even well inside the TTL.
    let other = meter_fleet(
        &changed,
        &home_str,
        "/",
        ReaderLimits::default(),
        60_000,
        now + 60_001,
        false,
        &mut cache,
    );
    assert!(!other.cached);
    assert_eq!(other.agents[0].agent.as_deref(), Some("a2"));

    // Force recomputes and restamps even on a cache hit path.
    let forced = meter_fleet(
        &agents,
        &home_str,
        "/",
        ReaderLimits::default(),
        60_000,
        now + 60_002,
        true,
        &mut cache,
    );
    assert!(!forced.cached);
    assert_eq!(forced.computed_at, now + 60_002);

    // Reset drops the cache entirely.
    cache.reset();
    let after_reset = meter_fleet(
        &agents,
        &home_str,
        "/",
        ReaderLimits::default(),
        60_000,
        now + 60_003,
        false,
        &mut cache,
    );
    assert!(!after_reset.cached);
    assert_eq!(after_reset.computed_at, now + 60_003);
}

#[test]
fn native_metering_reader_limits_from_env() {
    use hagency_metering::reader::ReaderLimits;

    let defaults = ReaderLimits::default();
    assert_eq!(defaults.window_ms, 30 * 24 * 60 * 60 * 1000);
    assert_eq!(defaults.max_files, 200);
    assert_eq!(defaults.max_bytes, 8 * 1024 * 1024);
    assert_eq!(defaults.max_entries, 20_000);

    let parsed = ReaderLimits::from_env(Some("1000"), Some("5"), Some("4096"), Some("10"));
    assert_eq!(
        parsed,
        ReaderLimits {
            window_ms: 1000,
            max_files: 5,
            max_bytes: 4096,
            max_entries: 10,
        }
    );

    // The JavaScript rule: a positive integer wins, anything else keeps the
    // default. parseInt tolerates leading whitespace and trailing garbage.
    assert_eq!(
        ReaderLimits::from_env(Some("  2000abc"), None, None, None).window_ms,
        2000
    );
    assert_eq!(
        ReaderLimits::from_env(Some("+300"), None, None, None).window_ms,
        300
    );
    assert_eq!(
        ReaderLimits::from_env(Some("0"), None, None, None).window_ms,
        defaults.window_ms,
        "zero is not positive"
    );
    assert_eq!(
        ReaderLimits::from_env(Some("-5"), None, None, None).window_ms,
        defaults.window_ms,
        "negative is not positive"
    );
    assert_eq!(
        ReaderLimits::from_env(Some(""), None, None, None).window_ms,
        defaults.window_ms
    );
    assert_eq!(
        ReaderLimits::from_env(Some("abc"), None, None, None).window_ms,
        defaults.window_ms
    );
    assert_eq!(
        ReaderLimits::from_env(Some("2.9"), None, None, None).window_ms,
        2,
        "parseInt stops at the dot"
    );
    assert_eq!(
        ReaderLimits::from_env(None, None, None, None),
        defaults,
        "absent keeps every default"
    );
}
