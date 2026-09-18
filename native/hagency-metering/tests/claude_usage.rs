use hagency_metering::{
    Framework, MAX_TOKEN_COUNT, MeteringError, TokenCounts,
    claude_usage::{ClaudeUsage, Coverage},
    observation::UsageObservation,
    runtime_usage::ProjectionDiagnostics,
};
fn input() -> ClaudeUsage {
    ClaudeUsage {
        counts: TokenCounts {
            input: Some(10),
            output: Some(20),
            cache_read: Some(30),
            cache_write: Some(40),
        },
        coverage: Coverage::ReportedModelsResult,
        steps: 2,
        models: 2,
        diagnostics: ProjectionDiagnostics::default(),
    }
}
fn encoded(input: ClaudeUsage) -> serde_json::Value {
    serde_json::to_value(UsageObservation::claude_runtime(input).unwrap()).unwrap()
}
#[test]
fn native_metering_claude_runtime() {
    let original = input();
    let observation = UsageObservation::claude_runtime(original).unwrap();
    assert_eq!(observation.framework(), Framework::Claude);
    assert!(observation.incomplete());
    assert_eq!(
        observation.counts().unwrap().display_volume().unwrap(),
        Some(100)
    );
    assert!(observation.runtime_evidence().is_none());
    assert!(observation.diagnostics().is_none());
    let evidence = observation.claude_runtime_evidence().unwrap();
    assert_eq!(evidence.version(), 1);
    assert!(evidence.stream_incomplete());
    assert!(evidence.usage() == &original);
    let first = encoded(original);
    assert!(first.get("runtime_evidence").is_none());
    for mode in [
        "input",
        "output",
        "read",
        "write",
        "coverage",
        "steps",
        "models",
        "missing",
        "invalid",
        "unsupported",
    ] {
        let mut changed = original;
        match mode {
            "input" => changed.counts.input = Some(11),
            "output" => changed.counts.output = Some(21),
            "read" => changed.counts.cache_read = Some(31),
            "write" => changed.counts.cache_write = Some(41),
            "coverage" => {
                changed.coverage = Coverage::MainLoopResult;
                changed.models = 0;
            }
            "steps" => changed.steps = 3,
            "models" => changed.models = 3,
            "missing" => changed.diagnostics.missing = true,
            "invalid" => changed.diagnostics.invalid = true,
            "unsupported" => changed.diagnostics.unsupported = true,
            _ => unreachable!(),
        };
        assert_ne!(
            first["snapshot_digest"],
            encoded(changed)["snapshot_digest"],
            "{mode}"
        );
    }
    let mut partial = original;
    partial.coverage = Coverage::MainLoopSteps;
    partial.models = 0;
    let observed = UsageObservation::claude_runtime(partial).unwrap();
    assert_eq!(observed.counts().unwrap().output, None);
    partial.counts.output = Some(999);
    assert_eq!(serde_json::to_value(observed).unwrap(), encoded(partial));
    let mut unsafe_input = original;
    unsafe_input.counts.input = Some(MAX_TOKEN_COUNT + 1);
    let a = encoded(unsafe_input);
    unsafe_input.counts.input = Some(u64::MAX);
    assert_eq!(a, encoded(unsafe_input));
    assert_eq!(a["totals"]["input"], serde_json::Value::Null);
    assert_eq!(
        a["claude_runtime_evidence"]["usage"]["diagnostics"]["invalid"],
        true
    );
    let mut overflow = original;
    overflow.counts.input = Some(MAX_TOKEN_COUNT);
    overflow.counts.output = None;
    assert!(matches!(
        UsageObservation::claude_runtime(overflow),
        Err(MeteringError::Overflow)
    ));
    for (steps, models) in [(1025, 2), (2, 65)] {
        let mut bad = original;
        bad.steps = steps;
        bad.models = models;
        assert!(matches!(
            UsageObservation::claude_runtime(bad),
            Err(MeteringError::Capacity)
        ));
    }
    for framework in [Framework::Claude, Framework::Codex] {
        let legacy = serde_json::to_value(UsageObservation::parse(framework, "").unwrap()).unwrap();
        assert!(legacy.get("runtime_evidence").is_none());
        assert!(legacy.get("claude_runtime_evidence").is_none());
    }
}
