use hagency_metering::{
    Framework, MAX_TOKEN_COUNT, MeteringError, TokenCounts,
    observation::UsageObservation,
    runtime_usage::{CodexUsage, CounterBreakdown, ProjectionDiagnostics},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn pinned() -> CodexUsage {
    CodexUsage {
        // Actual pinned codex-api SSE conversion fixture, not a transcript.
        total: CounterBreakdown {
            total_tokens: Some(110),
            input_tokens: Some(100),
            cached_input_tokens: Some(40),
            cache_write_input_tokens: Some(60),
            output_tokens: Some(10),
            reasoning_output_tokens: Some(5),
        },
        last: CounterBreakdown {
            total_tokens: Some(34),
            input_tokens: Some(30),
            cached_input_tokens: Some(10),
            cache_write_input_tokens: Some(5),
            output_tokens: Some(4),
            reasoning_output_tokens: Some(1),
        },
        context_window: Some(128_000),
        diagnostics: ProjectionDiagnostics::default(),
    }
}
fn encoded(usage: CodexUsage) -> Value {
    serde_json::to_value(UsageObservation::codex_runtime(usage).unwrap()).unwrap()
}
fn field(usage: &mut CodexUsage, index: usize) -> &mut Option<u64> {
    let counters = if index < 6 {
        &mut usage.total
    } else {
        &mut usage.last
    };
    match index % 6 {
        0 => &mut counters.total_tokens,
        1 => &mut counters.input_tokens,
        2 => &mut counters.cached_input_tokens,
        3 => &mut counters.cache_write_input_tokens,
        4 => &mut counters.output_tokens,
        _ => &mut counters.reasoning_output_tokens,
    }
}

#[test]
fn native_metering_runtime_pinned_categories() {
    let usage = pinned();
    let observation = UsageObservation::codex_runtime(usage).unwrap();
    let counts = observation.counts().unwrap();
    assert_eq!(
        counts,
        TokenCounts {
            input: Some(0),
            output: Some(10),
            cache_write: Some(60),
            cache_read: Some(40),
        }
    );
    assert_eq!(counts.display_volume().unwrap(), Some(110));
    assert_eq!(counts.ceiling_volume().unwrap(), Some(70));
    assert_eq!(observation.framework(), Framework::Codex);
    assert!(observation.incomplete());
    assert!(observation.diagnostics().is_none()); // No fabricated transcript parse.
    assert!(observation.failure().is_none());
    let evidence = observation.runtime_evidence().unwrap();
    assert_eq!(evidence.version(), 1);
    assert!(evidence.stream_incomplete());
    assert!(evidence.usage() == &usage);
    assert!(!evidence.normalization().total_inconsistent);
    assert!(!evidence.normalization().last_inconsistent);

    let mut changed_non_consumption = usage;
    changed_non_consumption.context_window = Some(MAX_TOKEN_COUNT);
    changed_non_consumption.last = usage.total;
    changed_non_consumption.total.reasoning_output_tokens = Some(0);
    assert_eq!(
        UsageObservation::codex_runtime(changed_non_consumption)
            .unwrap()
            .counts(),
        Some(counts)
    );
}

#[test]
fn native_metering_runtime_unknown_and_invalid() {
    let empty = UsageObservation::codex_runtime(CodexUsage::default()).unwrap();
    assert_eq!(
        empty.counts(),
        Some(TokenCounts {
            input: None,
            output: None,
            cache_write: None,
            cache_read: None,
        })
    );
    assert!(
        empty
            .runtime_evidence()
            .unwrap()
            .usage()
            .diagnostics
            .missing
    );
    assert!(empty.incomplete());

    for index in 0..12 {
        let mut usage = pinned();
        *field(&mut usage, index) = Some(u64::MAX);
        let observation = UsageObservation::codex_runtime(usage).unwrap();
        let evidence = observation.runtime_evidence().unwrap();
        let mut sanitized = *evidence.usage();
        assert!(field(&mut sanitized, index).is_none());
        assert!(sanitized.diagnostics.invalid);
        assert!(sanitized.diagnostics.missing);
        if index != 4 {
            assert_eq!(observation.counts().unwrap().output, Some(10));
        }
        // No independent valid value gets rounded or zeroed.
        for other in 0..12 {
            if other != index {
                assert_eq!(field(&mut sanitized, other), field(&mut pinned(), other));
            }
        }
    }
    let mut usage = pinned();
    usage.context_window = Some(MAX_TOKEN_COUNT + 1);
    usage.diagnostics.unsupported = true;
    let observation = UsageObservation::codex_runtime(usage).unwrap();
    let evidence = observation.runtime_evidence().unwrap();
    assert!(evidence.usage().context_window.is_none());
    assert!(evidence.usage().diagnostics.invalid);
    assert!(evidence.usage().diagnostics.unsupported);
    assert_eq!(
        observation.counts(),
        UsageObservation::codex_runtime(pinned()).unwrap().counts()
    );

    for index in 0..12 {
        let mut usage = pinned();
        *field(&mut usage, index) = None;
        let observation = UsageObservation::codex_runtime(usage).unwrap();
        assert!(
            observation
                .runtime_evidence()
                .unwrap()
                .usage()
                .diagnostics
                .missing
        );
        assert!(
            !observation
                .runtime_evidence()
                .unwrap()
                .usage()
                .diagnostics
                .invalid
        );
    }
}

#[test]
fn native_metering_runtime_contradictions_and_resets() {
    let mut impossible = pinned();
    impossible.total.cache_write_input_tokens = Some(61);
    impossible.total.reasoning_output_tokens = Some(11);
    impossible.total.total_tokens = Some(111);
    let observation = UsageObservation::codex_runtime(impossible).unwrap();
    assert_eq!(observation.counts().unwrap().input, None);
    assert_eq!(observation.counts().unwrap().cache_write, Some(61));
    assert_eq!(observation.counts().unwrap().output, Some(10));
    assert!(
        observation
            .runtime_evidence()
            .unwrap()
            .normalization()
            .total_inconsistent
    );

    let mut missing = pinned();
    missing.total.cached_input_tokens = None;
    missing.total.cache_write_input_tokens = Some(101);
    let observation = UsageObservation::codex_runtime(missing).unwrap();
    assert!(
        observation
            .runtime_evidence()
            .unwrap()
            .normalization()
            .total_inconsistent
    );
    assert_eq!(observation.counts().unwrap().input, None);

    let mut estimated = pinned();
    estimated.total = CounterBreakdown {
        total_tokens: Some(128_000),
        input_tokens: Some(0),
        cached_input_tokens: Some(0),
        cache_write_input_tokens: Some(0),
        output_tokens: Some(0),
        reasoning_output_tokens: Some(0),
    };
    estimated.last = estimated.total;
    let observation = UsageObservation::codex_runtime(estimated).unwrap();
    assert_eq!(
        observation.counts().unwrap().display_volume().unwrap(),
        Some(0)
    );
    let evidence = observation.runtime_evidence().unwrap();
    assert!(evidence.normalization().total_inconsistent);
    assert!(evidence.normalization().last_inconsistent);
    assert_eq!(evidence.usage().total.total_tokens, Some(128_000));
    assert!(observation.incomplete());

    // Each value remains the observed cumulative snapshot. Repetition/growth/
    // regression is not summed or repaired; the ledger owns those comparisons.
    for multiplier in [1, 1, 2, 1] {
        let mut usage = pinned();
        for index in 0..6 {
            let value = field(&mut usage, index);
            *value = value.map(|value| value * multiplier);
        }
        let observation = UsageObservation::codex_runtime(usage).unwrap();
        assert_eq!(
            observation.counts().unwrap().display_volume().unwrap(),
            Some(110 * multiplier)
        );
        assert!(
            !observation
                .runtime_evidence()
                .unwrap()
                .normalization()
                .total_inconsistent
        );
    }
}

#[test]
fn native_metering_runtime_digest_and_bounds() {
    let baseline = encoded(pinned());
    assert_eq!(baseline, encoded(pinned()));
    for index in 0..12 {
        let mut usage = pinned();
        let value = field(&mut usage, index);
        *value = value.map(|value| value + 1);
        assert_ne!(
            baseline["snapshot_digest"],
            encoded(usage)["snapshot_digest"]
        );
    }
    for flag in 0..4 {
        let mut usage = pinned();
        match flag {
            0 => usage.context_window = Some(128_001),
            1 => usage.diagnostics.missing = true,
            2 => usage.diagnostics.invalid = true,
            _ => usage.diagnostics.unsupported = true,
        }
        assert_ne!(
            baseline["snapshot_digest"],
            encoded(usage)["snapshot_digest"]
        );
    }
    let mut invalid_a = pinned();
    invalid_a.total.input_tokens = Some(MAX_TOKEN_COUNT + 1);
    let mut invalid_b = invalid_a;
    invalid_b.total.input_tokens = Some(u64::MAX);
    assert_eq!(encoded(invalid_a), encoded(invalid_b)); // Same sanitized evidence.

    let mut maximum = pinned();
    maximum.total = CounterBreakdown {
        input_tokens: Some(MAX_TOKEN_COUNT),
        cached_input_tokens: Some(0),
        cache_write_input_tokens: Some(0),
        output_tokens: Some(0),
        reasoning_output_tokens: Some(0),
        total_tokens: Some(MAX_TOKEN_COUNT),
    };
    assert_eq!(
        UsageObservation::codex_runtime(maximum)
            .unwrap()
            .counts()
            .unwrap()
            .display_volume()
            .unwrap(),
        Some(MAX_TOKEN_COUNT)
    );
    maximum.total.output_tokens = Some(1);
    assert!(matches!(
        UsageObservation::codex_runtime(maximum),
        Err(MeteringError::Overflow)
    ));
    maximum.total.input_tokens = None;
    maximum.total.cached_input_tokens = Some(MAX_TOKEN_COUNT);
    // Unknown fresh input must not hide overflow of known categories.
    assert!(matches!(
        UsageObservation::codex_runtime(maximum),
        Err(MeteringError::Overflow)
    ));
    assert_eq!(baseline["runtime_evidence"]["version"], 1);
    assert_eq!(baseline["runtime_evidence"]["stream_incomplete"], true);
}

#[test]
fn native_metering_runtime_legacy_shape() {
    let transcript = r#"{"payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"cache_write_input_tokens":60,"output_tokens":10,"reasoning_output_tokens":5,"total_tokens":110}}}}"#;
    let observation = UsageObservation::parse(Framework::Codex, transcript).unwrap();
    assert!(!observation.incomplete());
    assert!(observation.runtime_evidence().is_none());
    assert_eq!(
        serde_json::to_value(observation).unwrap(),
        json!({
            "framework":"codex",
            "snapshot_digest":format!("{:x}", Sha256::digest(transcript.as_bytes())),
            "totals":{"input":60,"output":10,"cacheWrite":0,"cacheRead":40},
            "diagnostics":{"malformedLines":0,"missingUsageRecords":0,"missingFields":0,
                "ambiguousWorkspace":false,"undeduplicableMessages":0,"nonMonotonic":0,"inconsistentRecords":0},
            "failure":null
        })
    );
}
