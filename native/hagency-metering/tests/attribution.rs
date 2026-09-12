//! Oracle replay and native-pinned tests for transcript attribution search.
//!
//! The fixture is produced by `native/scripts/attribution-vectors.mjs`, which
//! executes the retained JavaScript (`lib/metering/attribute.js`) against
//! deterministic synthetic inputs. Shared vectors avoid inputs where the
//! native parser deliberately corrects JavaScript coercion (missing usage,
//! duplicate identities, unsafe counters); native-only assertions pin that
//! corrected behavior instead.

use hagency_metering::attribution::{
    AgentRow, ScanBounds, SessionText, TranscriptSearch, agent_transcript_workspace,
    claude_project_dir, meter_agent, summarize_fleet, transcript_search,
};
use serde_json::{Value, json};

const PROCESS_CWD: &str = "/";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../fixtures/attribution.json")).unwrap()
}

fn session(file: &str, text: &str) -> SessionText {
    SessionText {
        file: file.to_owned(),
        text: text.to_owned(),
    }
}

/// Project a Rust `AgentRow` into the JavaScript row shape the fixture
/// records (camelCase keys, absent-on-unavailable fields).
fn js_row_projection(row: &AgentRow) -> Value {
    let mut value = serde_json::to_value(row).unwrap();
    let object = value.as_object_mut().unwrap();
    if !row.available {
        // The JavaScript rows never carry totals or file detail when
        // unavailable; `undefined` keys drop out of the JSON fixture.
        for key in ["totals", "total", "sessions", "skipped", "files"] {
            object.remove(key);
        }
        if row.workspace.is_none() {
            object.remove("workspace");
        }
    }
    value
}

/// Project a Rust `TranscriptSearch` into the JavaScript descriptor shape.
fn js_search_projection(search: &TranscriptSearch) -> Value {
    serde_json::to_value(search).unwrap()
}

fn claude_usage(id: &str, input: u64, output: u64, cache_write: u64, cache_read: u64) -> String {
    json!({
        "cwd": "/fixture/work", "uuid": id,
        "message": {"model": "fixture", "usage": {
            "input_tokens": input, "output_tokens": output,
            "cache_creation_input_tokens": cache_write,
            "cache_read_input_tokens": cache_read,
        }},
    })
    .to_string()
}

fn codex_usage(input: u64, cached: u64, output: u64) -> String {
    json!({
        "payload": {"cwd": "/fixture/work", "type": "token_count", "info": {"total_token_usage": {
            "input_tokens": input, "cached_input_tokens": cached, "output_tokens": output,
            "reasoning_output_tokens": output / 2, "total_tokens": input + output,
        }}},
    })
    .to_string()
}

fn lines(records: &[String]) -> String {
    records.join("\n")
}

#[test]
fn native_metering_attribution_vectors() {
    let fixture = fixture();
    assert_eq!(fixture["source"], "lib/metering/attribute.js");
    let vectors = &fixture["vectors"];

    for vector in vectors["projectDir"].as_array().unwrap() {
        let workspace_path = match &vector["workspacePath"] {
            Value::Null => "",
            Value::String(text) => text.as_str(),
            other => panic!("unsupported workspacePath shape: {other}"),
        };
        assert_eq!(
            claude_project_dir(workspace_path).as_deref(),
            vector["expected"].as_str(),
            "projectDir vector {}",
            vector["name"]
        );
    }

    for vector in vectors["search"].as_array().unwrap() {
        let framework = vector["framework"].as_str().unwrap_or("");
        let workspace_path = vector["workspacePath"].as_str().unwrap_or("");
        let home_dir = vector["homeDir"].as_str().unwrap_or("");
        let expected = &vector["expected"];
        match transcript_search(framework, workspace_path, home_dir) {
            Some(search) => assert_eq!(
                js_search_projection(&search),
                *expected,
                "search vector {}",
                vector["name"]
            ),
            None => assert!(
                expected.is_null(),
                "search vector {} expected a descriptor",
                vector["name"]
            ),
        }
    }

    for vector in vectors["workspace"].as_array().unwrap() {
        let agent = &vector["agent"];
        let expected = &vector["expected"];
        let actual = agent_transcript_workspace(agent);
        let actual = match actual {
            Some(text) => Value::String(text),
            None => Value::Null,
        };
        assert_eq!(actual, *expected, "workspace vector {}", vector["name"]);
    }

    for vector in vectors["meterAgent"].as_array().unwrap() {
        let agent = &vector["agent"];
        let home_dir = vector["homeDir"].as_str().unwrap_or("");
        let sessions: Vec<SessionText> = vector["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                session(
                    entry["file"].as_str().unwrap(),
                    entry["text"].as_str().unwrap(),
                )
            })
            .collect();
        let bounds = match vector["bounds"].as_null() {
            Some(()) => None,
            None => Some(ScanBounds {
                dropped_by_count: vector["bounds"]["droppedByCount"].as_u64().unwrap_or(0),
                entries_unwalked: vector["bounds"]["entriesUnwalked"].as_u64().unwrap_or(0),
            }),
        };
        let row = meter_agent(agent, home_dir, PROCESS_CWD, &sessions, bounds);
        assert_eq!(
            js_row_projection(&row),
            vector["expected"],
            "meterAgent vector {}",
            vector["name"]
        );
    }

    for vector in vectors["fleet"].as_array().unwrap() {
        let rows: Vec<AgentRow> = vector["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| serde_json::from_value(row.clone()).unwrap())
            .collect();
        let summary = summarize_fleet(rows, PROCESS_CWD);
        let mut projected = serde_json::to_value(&summary).unwrap();
        // The JavaScript summary marks shared rows by clearing fields that
        // JSON.stringify drops as `undefined`; mirror that projection.
        for agent in projected["agents"].as_array_mut().unwrap() {
            if agent.get("available") == Some(&Value::Bool(false)) {
                for key in ["totals", "total"] {
                    if agent.get(key) == Some(&Value::Null) {
                        agent.as_object_mut().unwrap().remove(key);
                    }
                }
            }
        }
        assert_eq!(
            projected, vector["expected"],
            "fleet vector {}",
            vector["name"]
        );
    }
}

#[test]
fn native_metering_attribution_search_layout() {
    // Claude: one directory per workspace, narrowed, transcripts flat inside it.
    let claude = transcript_search("claude", "/Users/fixture/proj", "/Users/fixture/home")
        .expect("claude has a search");
    assert_eq!(
        claude,
        TranscriptSearch {
            dir: "/Users/fixture/home/.claude/projects/-Users-fixture-proj".to_owned(),
            narrowed: true,
            recursive: false,
        }
    );
    // Codex: filed by date under a nested tree, so nothing narrows and the
    // walk must descend — a flat walk finds zero files and is
    // indistinguishable from an agent that did no work.
    let codex = transcript_search("codex", "/Users/fixture/proj", "/Users/fixture/home")
        .expect("codex has a search");
    assert_eq!(
        codex,
        TranscriptSearch {
            dir: "/Users/fixture/home/.codex/sessions".to_owned(),
            narrowed: false,
            recursive: true,
        }
    );
    // Framework matching is case-insensitive, as in the JavaScript.
    assert_eq!(
        transcript_search("CLAUDE", "/w", "/h").map(|s| (s.narrowed, s.recursive)),
        Some((true, false))
    );
    assert_eq!(
        transcript_search("Codex", "/w", "/h").map(|s| (s.narrowed, s.recursive)),
        Some((false, true))
    );
    // A relative workspace has no project directory, so no search either.
    assert!(transcript_search("claude", "relative", "/h").is_none());
    // Unknown frameworks are unavailable, not assumed.
    assert!(transcript_search("something-new", "/w", "/h").is_none());
    assert!(transcript_search("", "/w", "/h").is_none());
    assert!(transcript_search("octos", "/w", "/h").is_none());
    // Unicode workspaces survive the forward mapping and the join.
    let unicode = transcript_search("claude", "/Users/fixture/工程", "/home").unwrap();
    assert_eq!(unicode.dir, "/home/.claude/projects/-Users-fixture-工程");
    // `path.join` semantics: dot segments normalize, empty parts drop.
    let dotted = transcript_search("claude", "/w", "/h/./x/../").unwrap();
    assert_eq!(dotted.dir, "/h/.claude/projects/-w");
    assert_eq!(claude_project_dir("/"), Some("-".to_owned()));
    assert_eq!(claude_project_dir("w"), None);
}

#[test]
fn native_metering_attribution_unavailable_never_zero() {
    let home = "/Users/fixture/home";
    let ws = "/fixture/work";

    // Every `available: false` reason, in the JavaScript's own words.
    let unsupported = meter_agent(
        &json!({"name": "o1", "type": "octos", "workspacePath": ws}),
        home,
        PROCESS_CWD,
        &[session("never-read", &claude_usage("u", 1, 2, 3, 4))],
        None,
    );
    assert!(!unsupported.available);
    assert_eq!(
        unsupported.reason.as_deref(),
        Some(
            "octos session files record no usage object and no cwd, so consumption can be neither counted nor attributed from them"
        )
    );
    assert!(unsupported.totals.is_none());
    assert!(unsupported.total.is_none());

    let unknown_framework = meter_agent(
        &json!({"name": "o4", "type": "something-new", "workspacePath": ws}),
        home,
        PROCESS_CWD,
        &[],
        None,
    );
    assert_eq!(
        unknown_framework.reason.as_deref(),
        Some("no metering adapter exists for this framework")
    );

    let no_workspace = meter_agent(
        &json!({"name": "a1", "type": "claude", "workspacePath": null, "workdir": ws}),
        home,
        PROCESS_CWD,
        &[session("never-read", &claude_usage("u", 1, 2, 3, 4))],
        None,
    );
    assert!(!no_workspace.available);
    assert_eq!(
        no_workspace.reason.as_deref(),
        Some(
            "no workspace recorded for this agent, so its transcripts cannot be located; a running agent reports one and a stopped one may never have"
        )
    );
    // `workdir` is a search key, not an observation: alone it must not meter.
    assert!(no_workspace.workspace.is_none());

    let other_workspace = meter_agent(
        &json!({"name": "a2", "type": "claude", "workspacePath": ws}),
        home,
        PROCESS_CWD,
        &[session(
            "elsewhere.jsonl",
            &claude_usage("u", 1, 2, 3, 4).replace("/fixture/work", "/fixture/other"),
        )],
        None,
    );
    assert!(!other_workspace.available);
    assert_eq!(
        other_workspace.reason.as_deref(),
        Some("opened 1 transcript(s), none of which recorded this workspace")
    );
    assert!(other_workspace.totals.is_none());

    // A genuinely empty scan keeps the exact "none found yet" reason.
    let empty = meter_agent(
        &json!({"name": "fresh", "type": "codex", "lastWorkspacePath": "/fixture/new"}),
        home,
        PROCESS_CWD,
        &[],
        None,
    );
    assert_eq!(
        empty.reason.as_deref(),
        Some("no transcripts found for this workspace yet")
    );

    // Both zero-match facts compose additively, not as an if/else chain.
    let both = meter_agent(
        &json!({"name": "b2", "type": "codex", "lastWorkspacePath": "/fixture/work"}),
        home,
        PROCESS_CWD,
        &[
            session(
                "busy-a.jsonl",
                &lines(&[
                    json!({"type":"session_meta","payload":{"cwd":"/fixture/other"}}).to_string(),
                    codex_usage(10, 0, 5),
                ]),
            ),
            session(
                "busy-b.jsonl",
                &lines(&[
                    json!({"type":"session_meta","payload":{"cwd":"/fixture/other"}}).to_string(),
                    codex_usage(10, 0, 5),
                ]),
            ),
        ],
        Some(ScanBounds {
            dropped_by_count: 380,
            entries_unwalked: 7,
        }),
    );
    assert!(!both.available);
    let reason = both.reason.as_deref().expect("zero-match reason");
    assert!(reason.starts_with("opened 2 transcript(s), none of which recorded this workspace"));
    assert!(reason.contains("; and "));
    assert!(reason.contains("387 further candidate transcript(s) were never opened"));
    assert!(reason.contains("not evidence the agent did no work"));
    assert!(both.totals.is_none());
    assert!(both.total.is_none());

    // Bounds are ignored once a session matched: the figure is attributed.
    let attributed = meter_agent(
        &json!({"name": "b4", "type": "codex", "lastWorkspacePath": "/fixture/work"}),
        home,
        PROCESS_CWD,
        &[session("mine.jsonl", &codex_usage(70, 7, 30))],
        Some(ScanBounds {
            dropped_by_count: 5,
            entries_unwalked: 5,
        }),
    );
    assert!(attributed.available);
    assert_eq!(attributed.total, Some(100));
    assert_eq!(attributed.reason, None);

    // A session the native parser refuses is skipped, never coerced to a
    // number: duplicate JSON keys fail the whole observation.
    let refused = meter_agent(
        &json!({"name": "a5", "type": "claude", "workspacePath": "/fixture/work"}),
        home,
        PROCESS_CWD,
        &[session(
            "dup.jsonl",
            "{\"cwd\":\"/fixture/work\",\"uuid\":\"u\",\"uuid\":\"v\"}",
        )],
        None,
    );
    assert!(!refused.available);
    assert_eq!(
        refused.reason.as_deref(),
        Some("opened 1 transcript(s), none of which recorded this workspace")
    );

    // Native correction, pinned: missing usage stays unknown, never zero.
    // The JavaScript oracle coerces these to 0; shared vectors avoid them.
    let missing = meter_agent(
        &json!({"name": "a6", "type": "claude", "workspacePath": "/fixture/work"}),
        home,
        PROCESS_CWD,
        &[session(
            "missing.jsonl",
            &json!({
                "cwd": "/fixture/work", "uuid": "u1",
                "message": {"usage": {"input_tokens": 5}}
            })
            .to_string(),
        )],
        None,
    );
    assert!(missing.available);
    let totals = missing.totals.expect("row remains attributable");
    assert_eq!(totals.input, Some(5));
    assert_eq!(totals.output, None);
    assert_eq!(totals.cache_write, None);
    assert_eq!(totals.cache_read, None);
    assert_eq!(missing.total, None, "a partial measurement is not a total");

    // Fleet summary: an unavailable agent yields null, never zero, and the
    // shared-workspace row keeps its own reason while losing its totals.
    let ok_row = AgentRow {
        agent: Some("a1".to_owned()),
        available: true,
        framework: "claude".to_owned(),
        workspace: Some("/fixture/ws".to_owned()),
        totals: Some(hagency_metering::TokenCounts {
            input: Some(1),
            output: Some(2),
            cache_write: Some(3),
            cache_read: Some(4),
        }),
        total: Some(10),
        sessions: Some(1),
        skipped: Some(0),
        files: Some(Vec::new()),
        reason: None,
    };
    let unavailable_row = AgentRow {
        agent: Some("a2".to_owned()),
        available: false,
        framework: "claude".to_owned(),
        workspace: None,
        totals: None,
        total: None,
        sessions: None,
        skipped: None,
        files: None,
        reason: Some("no workspace recorded for this agent, so its transcripts cannot be located; a running agent reports one and a stopped one may never have".to_owned()),
    };
    let summary = summarize_fleet(vec![ok_row, unavailable_row], PROCESS_CWD);
    assert_eq!(summary.attributed, 1);
    assert_eq!(summary.unattributed, 1);
    assert_eq!(summary.total, Some(10));
    assert_eq!(
        summary.reason.as_deref(),
        Some("1 of 2 agents could not be attributed")
    );
    let nothing = summarize_fleet(
        vec![AgentRow {
            agent: Some("a1".to_owned()),
            available: false,
            framework: "claude".to_owned(),
            workspace: None,
            totals: None,
            total: None,
            sessions: None,
            skipped: None,
            files: None,
            reason: Some("no transcripts found for this workspace yet".to_owned()),
        }],
        PROCESS_CWD,
    );
    assert_eq!(
        nothing.total, None,
        "nothing attributable yields null, never zero"
    );
    assert_eq!(nothing.totals, None);
    assert_eq!(nothing.attributed, 0);
}
