use hagency_metering::{
    Framework, MAX_IDENTITIES, MAX_LINE_BYTES, MAX_LINES, MAX_SNAPSHOT_BYTES, MAX_TOKEN_COUNT,
    MeteringError, SessionDetails, SessionReport, TokenCounts, parse_session,
};
use serde_json::{Value, json};

fn claude(id: Value, usage: Value) -> Value {
    json!({"uuid":id,"cwd":"/fixture/work","message":{"model":"fixture","usage":usage}})
}
fn usage(input: u64, output: u64, write: u64, read: u64) -> Value {
    json!({"input_tokens":input,"output_tokens":output,"cache_creation_input_tokens":write,"cache_read_input_tokens":read})
}
fn codex(input: u64, cached: u64, output: u64, reasoning: u64, total: u64) -> Value {
    json!({"payload":{"cwd":"/fixture/work","info":{"total_token_usage":{"input_tokens":input,"cached_input_tokens":cached,"output_tokens":output,"reasoning_output_tokens":reasoning,"total_tokens":total}}}})
}
fn lines(records: &[Value]) -> String {
    records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
fn legacy_projection(report: &SessionReport) -> Value {
    match &report.details {
        SessionDetails::Claude { messages, models } => json!({
            "framework":"claude","cwd":report.workspace_hint,"totals":report.totals,
            "messages":messages,"models":models,"undedupable":report.diagnostics.undeduplicable_messages,
        }),
        SessionDetails::Codex {
            turns,
            reasoning_output,
            cumulative_total,
            agrees_with_cli,
        } => json!({
            "framework":"codex","cwd":report.workspace_hint,"totals":report.totals,
            "turns":turns,"reasoningOutput":reasoning_output,"cumulativeTotal":cumulative_total,
            "agreesWithCli":agrees_with_cli,"nonMonotonic":report.diagnostics.non_monotonic,
        }),
    }
}

#[test]
fn native_metering_legacy_vectors() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/metering.json")).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 135);
    for vector in vectors {
        let framework = match vector["framework"].as_str().unwrap() {
            "claude" => Framework::Claude,
            "codex" => Framework::Codex,
            _ => panic!("unsupported fixture"),
        };
        let report = parse_session(framework, vector["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            legacy_projection(&report),
            vector["expected"],
            "{}",
            vector["name"]
        );
    }
}

#[test]
fn native_metering_incomplete_observations() {
    let absent = parse_session(
        Framework::Claude,
        &lines(&[
            claude(json!("first"), usage(1, 2, 3, 4)),
            json!({"type":"assistant","uuid":"second","message":{"role":"assistant","content":[]}}),
            json!({"type":"user","message":{"content":"no usage expected"}}),
        ]),
    )
    .unwrap();
    assert_eq!(absent.diagnostics.missing_usage_records, 1);
    let absent = parse_session(
        Framework::Codex,
        &lines(&[
            codex(10, 1, 5, 2, 15),
            json!({"type":"event_msg","payload":{"type":"token_count","info":null}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"not usage"}}),
        ]),
    )
    .unwrap();
    assert_eq!(absent.diagnostics.missing_usage_records, 1);
    for framework in [Framework::Claude, Framework::Codex] {
        let report = parse_session(framework, "\n{}\n").unwrap();
        assert!(report.totals.is_none());
        assert!(report.workspace_hint.is_none());
    }
    let input = lines(&[
        claude(json!("first"), usage(1, 2, 3, 4)),
        claude(json!("second"), json!({"output_tokens":2})),
    ]);
    let report = parse_session(Framework::Claude, &input).unwrap();
    let totals = report.totals.unwrap();
    assert_eq!(totals.input, None);
    assert_eq!(totals.output, Some(4));
    assert_eq!(totals.display_volume().unwrap(), None);
    assert_eq!(report.diagnostics.missing_fields, 3);

    let report = parse_session(
        Framework::Claude,
        &format!(
            "bad json\n{}\n{{\"unfinished\":",
            claude(Value::Null, usage(1, 2, 3, 4))
        ),
    )
    .unwrap();
    assert_eq!(report.diagnostics.malformed_lines, 2);
    assert_eq!(report.diagnostics.undeduplicable_messages, 1);

    let report = parse_session(
        Framework::Claude,
        &lines(&[
            json!({"cwd":"/fixture/first"}),
            json!({"cwd":"/fixture/second"}),
            claude(json!("a"), usage(1, 2, 3, 4)),
        ]),
    )
    .unwrap();
    assert!(report.diagnostics.ambiguous_workspace);
    assert!(report.workspace_hint.is_none());

    let report = parse_session(
        Framework::Codex,
        r#"{"payload":{"info":{"total_token_usage":{"output_tokens":0}}}}"#,
    )
    .unwrap();
    assert_eq!(report.totals.unwrap().input, None);
    assert!(matches!(
        report.details,
        SessionDetails::Codex {
            turns: None,
            cumulative_total: None,
            agrees_with_cli: None,
            ..
        }
    ));
    assert_eq!(report.diagnostics.missing_fields, 4);
}

#[test]
fn native_metering_conflicting_observations() {
    let report = parse_session(
        Framework::Codex,
        &lines(&[
            codex(900, 500, 100, 20, 1000),
            json!({"payload":{"info":{"total_token_usage":{}}}}),
            codex(800, 500, 300, 20, 1100),
        ]),
    )
    .unwrap();
    assert_eq!(report.diagnostics.inconsistent_records, 1);
    assert_eq!(report.diagnostics.missing_fields, 5);
    for (first, second) in [
        (
            codex(900, 500, 100, 20, 1000),
            codex(800, 500, 200, 20, 1000),
        ),
        (
            codex(900, 500, 100, 20, 1000),
            codex(800, 500, 300, 20, 1100),
        ),
        (
            codex(900, 500, 100, 20, 1000),
            codex(1000, 400, 100, 20, 1100),
        ),
    ] {
        let report = parse_session(Framework::Codex, &lines(&[first, second])).unwrap();
        assert_eq!(report.diagnostics.inconsistent_records, 1);
        assert_eq!(report.diagnostics.non_monotonic, 0);
    }
    for text in [
        r#"{"payload":{},"payload":{}}"#,
        r#"{"unrelated":{"x":1,"x":2}}"#,
        r#"{"message":{"usage":{"input_tokens":1,"input_tokens":2}}}"#,
    ] {
        assert_eq!(
            parse_session(Framework::Claude, text).unwrap_err(),
            MeteringError::DuplicateKey
        );
    }
    let same = claude(json!("same"), usage(1, 2, 3, 4));
    let report = parse_session(Framework::Claude, &lines(&[same.clone(), same.clone()])).unwrap();
    assert_eq!(report.totals.unwrap().output, Some(2));
    assert_eq!(
        parse_session(
            Framework::Claude,
            &lines(&[same, claude(json!("same"), usage(1, 3, 3, 4))])
        )
        .unwrap_err(),
        MeteringError::ConflictingIdentity
    );

    let report = parse_session(
        Framework::Codex,
        &lines(&[codex(100, 90, 20, 10, 120), codex(50, 40, 10, 5, 60)]),
    )
    .unwrap();
    assert_eq!(report.diagnostics.non_monotonic, 1);
    assert_eq!(report.totals.unwrap().display_volume().unwrap(), Some(60));
    let report = parse_session(Framework::Codex, &codex(10, 20, 5, 6, 99).to_string()).unwrap();
    assert_eq!(report.diagnostics.inconsistent_records, 1);
    assert_eq!(report.totals.unwrap().input, None);
    assert!(matches!(
        report.details,
        SessionDetails::Codex {
            agrees_with_cli: Some(false),
            ..
        }
    ));
}

#[test]
fn native_metering_bounds() {
    for literal in [
        "-0",
        "1.0",
        "1e0",
        "1e309",
        "9007199254740992",
        "18446744073709551616",
    ] {
        let input = format!(r#"{{"message":{{"usage":{{"input_tokens":{literal}}}}}}}"#);
        assert_eq!(
            parse_session(Framework::Claude, &input).unwrap_err(),
            MeteringError::InvalidCounter,
            "{literal}"
        );
    }
    for (first, second) in [
        (
            usage(MAX_TOKEN_COUNT, 1, 0, 0),
            json!({"input_tokens":null,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}),
        ),
        (
            usage(MAX_TOKEN_COUNT, 0, 0, 0),
            json!({"input_tokens":null,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}),
        ),
    ] {
        assert_eq!(
            parse_session(
                Framework::Claude,
                &lines(&[claude(json!("a"), first), claude(json!("b"), second)])
            )
            .unwrap_err(),
            MeteringError::Overflow
        );
    }
    for token in [
        json!(-1),
        json!(1.5),
        json!("100"),
        json!(true),
        json!(MAX_TOKEN_COUNT + 1),
    ] {
        let mut record = claude(json!("a"), usage(0, 0, 0, 0));
        record["message"]["usage"]["input_tokens"] = token;
        assert_eq!(
            parse_session(Framework::Claude, &record.to_string()).unwrap_err(),
            MeteringError::InvalidCounter
        );
    }
    let max = claude(json!("a"), usage(MAX_TOKEN_COUNT, 0, 0, 0));
    assert_eq!(
        parse_session(Framework::Claude, &max.to_string())
            .unwrap()
            .totals
            .unwrap()
            .display_volume()
            .unwrap(),
        Some(MAX_TOKEN_COUNT)
    );
    assert_eq!(
        parse_session(
            Framework::Claude,
            &lines(&[max, claude(json!("b"), usage(1, 0, 0, 0))])
        )
        .unwrap_err(),
        MeteringError::Overflow
    );
    for input in [
        " ".repeat(MAX_SNAPSHOT_BYTES + 1),
        " ".repeat(MAX_LINE_BYTES + 1),
        "\n".repeat(MAX_LINES),
        format!("{}0{}", "[".repeat(33), "]".repeat(33)),
        json!({"nested":vec![0;4096]}).to_string(),
        json!({"cwd":"a".repeat(4097)}).to_string(),
        claude(json!("a".repeat(257)), usage(0, 0, 0, 0)).to_string(),
    ] {
        assert_eq!(
            parse_session(Framework::Claude, &input).unwrap_err(),
            MeteringError::Capacity
        );
    }
    let mut input = String::new();
    for index in 0..MAX_IDENTITIES {
        input.push_str(&claude(json!(index.to_string()), usage(0, 0, 0, 0)).to_string());
        input.push('\n');
    }
    assert!(parse_session(Framework::Claude, &input).is_ok());
    input.push_str(&claude(json!("extra"), usage(0, 0, 0, 0)).to_string());
    assert_eq!(
        parse_session(Framework::Claude, &input).unwrap_err(),
        MeteringError::Capacity
    );

    let models: Vec<_> = (0..65)
        .map(|i| {
            let mut record = claude(json!(i.to_string()), usage(0, 0, 0, 0));
            record["message"]["model"] = json!(i.to_string());
            record
        })
        .collect();
    assert_eq!(
        parse_session(Framework::Claude, &lines(&models)).unwrap_err(),
        MeteringError::Capacity
    );
    let workspaces: Vec<_> = (0..17)
        .map(|i| json!({"cwd":format!("/fixture/{i}")}))
        .collect();
    assert_eq!(
        parse_session(Framework::Claude, &lines(&workspaces)).unwrap_err(),
        MeteringError::Capacity
    );
}

#[test]
fn native_metering_ceiling_categories() {
    let counts = TokenCounts {
        input: Some(19765),
        output: Some(300),
        cache_write: Some(20),
        cache_read: Some(4800089833),
    };
    assert_eq!(counts.display_volume().unwrap(), Some(4800109918));
    assert_eq!(counts.ceiling_volume().unwrap(), Some(20085));
    let unknown_cache = TokenCounts {
        cache_read: None,
        ..counts
    };
    assert_eq!(unknown_cache.display_volume().unwrap(), None);
    assert_eq!(unknown_cache.ceiling_volume().unwrap(), Some(20085));
    assert_eq!(
        TokenCounts {
            input: None,
            ..counts
        }
        .ceiling_volume()
        .unwrap(),
        None
    );
    assert_eq!(
        TokenCounts {
            input: Some(u64::MAX),
            ..counts
        }
        .ceiling_volume(),
        Err(MeteringError::Overflow)
    );
}
