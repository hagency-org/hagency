//! ADR-139: the bare `app-server` argv still yields stdio on the pinned Codex
//! CLI. Offline test against the recorded `--help` excerpt; it never skips —
//! a captured_version other than the spec pin (0.153.4) or a transport
//! default that is not stdio fails the test.
use std::path::Path;

const PINNED_CODEX: &str = "0.153.4";
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/codex-cli/app-server-help-0.153.4.excerpt.txt"
);

#[test]
fn native_codex_stdio_flag_matches_pinned_cli() {
    let path = Path::new(FIXTURE);
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("pinned CLI help excerpt unreadable at {FIXTURE}: {error}"));
    // The capture header names which binary produced the excerpt. Anything
    // other than the spec-pinned version fails — never skips — because the
    // recorded stdio default is only authoritative for the pinned build.
    let captured = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("# captured_version:"))
        .map(str::trim)
        .unwrap_or_else(|| panic!("no captured_version header in {FIXTURE}"));
    let version = captured.split_whitespace().next().unwrap_or_default();
    assert_eq!(
        version, PINNED_CODEX,
        "the excerpt was captured from Codex {version}, not the spec-pinned {PINNED_CODEX}; \
         re-capture the excerpt from a real {PINNED_CODEX} binary (or move the spec pin to \
         the version actually captured) before relying on its transport default"
    );
    // The transport assertions ADR-139 relies on: stdio is the `--listen`
    // default and `--stdio` is an exact synonym, so the bare argv yields stdio.
    assert!(
        text.contains("[default: stdio://]"),
        "the excerpt does not record stdio as the --listen default"
    );
    assert!(
        text.contains("--stdio") && text.contains("equivalent to `--listen stdio://`"),
        "the excerpt does not record --stdio as an exact synonym of --listen stdio://"
    );
    assert!(
        !text.contains("[default: ws://") && !text.contains("[default: unix"),
        "the excerpt records a non-stdio transport default"
    );
}
