use hagency_progress::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn verb(text: &str) -> Verb {
    match text {
        "read" => Verb::Read,
        "searched" => Verb::Searched,
        "ran commands" => Verb::Commands,
        "edited" => Verb::Edited,
        "wrote" => Verb::Wrote,
        "fetched" => Verb::Fetched,
        "worked" => Verb::Worked,
        _ => panic!("unvetted fixture verb"),
    }
}
fn kind(text: &str) -> Kind {
    match text {
        "start" => Kind::Start,
        "step" => Kind::Step,
        "done" => Kind::Done,
        _ => panic!("fixture kind"),
    }
}
fn hook_series(steps: &Value) -> Vec<Value> {
    let run = RunId::new("host-run".into()).unwrap();
    let mut state = Accumulator::new(run.clone(), Filter::default());
    steps
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let now = step["now"].as_u64().unwrap();
            state
                .observe_hook(
                    &run,
                    i as u64 + 1,
                    now,
                    &json!({"hook_event_name":step["event"],"tool_name":step.get("tool")}),
                )
                .unwrap();
            if let Some(emission) = state.claim(&run, now).unwrap() {
                state
                    .settle(
                        &run,
                        emission.id(),
                        now,
                        if step["accepted"] == true {
                            AttemptOutcome::ObservedAccepted
                        } else {
                            AttemptOutcome::ObservedNotAccepted
                        },
                    )
                    .unwrap();
                json!(emission.text())
            } else {
                Value::Null
            }
        })
        .collect()
}
fn acp_finished(input: &Value) -> String {
    let run = RunId::new("oracle-acp".into()).unwrap();
    let mut state = Accumulator::new(run.clone(), Filter::default());
    let updates = input["updates"].as_array().unwrap();
    for (i, update) in updates.iter().enumerate() {
        state
            .observe_acp(&run, i as u64 + 1, i as u64, update)
            .unwrap();
    }
    let delivery = input["delivered"]
        .as_u64()
        .map(|n| AnswerDelivery::new(n as u32, DeliveryProof::FinalReplyJournalInspection));
    let next = updates.len() as u64 + 1;
    state.finish(&run, next, next, delivery).unwrap();
    state.claim(&run, next).unwrap().unwrap().text().into()
}
#[test]
fn native_progress_vectors() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/progress.json")).unwrap();
    for (path, source) in [
        (
            "lib/progress-filter.js",
            include_str!("../../../lib/progress-filter.js"),
        ),
        (
            "bin/hagency-progress",
            include_str!("../../../bin/hagency-progress"),
        ),
        (
            "scripts/hagency-acp-agent.mjs",
            include_str!("../../../scripts/hagency-acp-agent.mjs"),
        ),
    ] {
        assert_eq!(
            fixture["hashes"][path],
            format!(
                "{:x}",
                Sha256::digest(source.replace("\r\n", "\n").as_bytes())
            )
        );
    }
    for case in fixture["vectors"].as_array().unwrap() {
        let input = &case["input"];
        let expected = &case["expected"];
        match case["operation"].as_str().unwrap() {
            "filter" => match Filter::parse(&input["raw"], input["group"].as_str()) {
                Ok(filter) => {
                    assert_eq!(expected["ok"], true, "{input}");
                    let e = &expected["filter"];
                    assert_eq!(
                        filter.min_interval_ms(),
                        e["minIntervalMs"].as_f64().unwrap()
                    );
                    assert_eq!(json!(filter.events()), e["events"]);
                    assert_eq!(json!(filter.included_tools()), e["tools"]["include"]);
                    assert_eq!(json!(filter.excluded_tools()), e["tools"]["exclude"]);
                    assert_eq!(
                        filter.source(),
                        match e["source"].as_str().unwrap() {
                            "default" => Source::Default,
                            "file" => Source::File,
                            _ => Source::PerGroup,
                        }
                    );
                }
                Err(Error::Config(reason)) => {
                    assert_eq!(expected["ok"], false);
                    assert_eq!(expected["reason"], reason);
                }
                Err(error) => panic!("unexpected fixture error: {error}"),
            },
            "decision" => {
                let filter = Filter::parse(&input["raw"], None).unwrap();
                assert_eq!(
                    serde_json::to_value(
                        filter.decide(input["event"].as_str(), input["tool"].as_str())
                    )
                    .unwrap(),
                    *expected
                );
            }
            "verb" => assert_eq!(
                Verb::for_tool(input.as_str().unwrap()).text(),
                expected.as_str().unwrap()
            ),
            "acp" => assert_eq!(
                json!({"mapped":acp_tool(input).map(|tool|json!({"event":"PostToolUse","tool":tool})),"failed":acp_failed(input)}),
                *expected
            ),
            "summary" => {
                let mut counts = Counts::default();
                for pair in input["pairs"].as_array().unwrap() {
                    counts
                        .add(
                            verb(pair[0].as_str().unwrap()),
                            pair[1].as_u64().unwrap() as u32,
                        )
                        .unwrap();
                }
                assert_eq!(
                    json!(build_summary(
                        kind(input["kind"].as_str().unwrap()),
                        &counts,
                        input["failures"].as_u64().unwrap() as u32,
                        input["delivered"].as_u64().map(|n| n as u32)
                    )),
                    *expected
                );
            }
            "acp_finish" => {
                assert_eq!(acp_finished(input), expected.as_str().unwrap());
            }
            "hook_series" => assert_eq!(
                hook_series(input),
                expected
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["emitted"].clone())
                    .collect::<Vec<_>>()
            ),
            _ => panic!("unrecognized oracle operation"),
        }
    }
}
#[test]
fn native_progress_vectors_explicit_corrections() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/progress.json")).unwrap();
    for case in fixture["corrections"].as_array().unwrap() {
        assert!(!case["rationale"].as_str().unwrap().is_empty());
        let input = &case["input"];
        match case["operation"].as_str().unwrap() {
            "filter" => assert!(matches!(
                Filter::parse(&input["raw"], input["group"].as_str()),
                Err(Error::Config("selected perGroup rule is not an object"))
            )),
            "verb" => assert_eq!(
                Verb::for_tool(input.as_str().unwrap()).text(),
                case["native"].as_str().unwrap()
            ),
            "acp" => assert_eq!(
                acp_failed(input),
                case["native"]["failed"].as_bool().unwrap()
            ),
            "finished_totals" => assert_eq!(hook_series(input).last().unwrap(), &case["native"]),
            "acp_outcomes" => assert_eq!(acp_finished(input), case["native"].as_str().unwrap()),
            "failed_call" => {
                let run = RunId::new("failed-run".into()).unwrap();
                let mut state = Accumulator::new(run.clone(), Filter::default());
                for (i, payload) in input.as_array().unwrap().iter().enumerate() {
                    state
                        .observe_acp(&run, i as u64 + 1, i as u64, payload)
                        .unwrap();
                }
                state.finish(&run, 4, 4, None).unwrap();
                assert_eq!(
                    state.claim(&run, 4).unwrap().unwrap().text(),
                    case["native"].as_str().unwrap()
                );
            }
            "excluded_failure" => {
                let run = RunId::new("exclude".into()).unwrap();
                let mut state = Accumulator::new(
                    run.clone(),
                    Filter::parse(&json!({"tools":{"exclude":["Bash"]}}), None).unwrap(),
                );
                for (i, update) in input.as_array().unwrap().iter().enumerate() {
                    state
                        .observe_acp(&run, i as u64 + 1, i as u64, update)
                        .unwrap();
                }
                state.finish(&run, 3, 3, None).unwrap();
                assert_eq!(
                    state.claim(&run, 3).unwrap().unwrap().text(),
                    case["native"].as_str().unwrap()
                );
            }
            "silent_filter" => {
                let run = RunId::new("silent".into()).unwrap();
                let mut state = Accumulator::new(run.clone(), Filter::parse(input, None).unwrap());
                state.finish(&run, 1, 1, None).unwrap();
                assert!(state.claim(&run, 1).unwrap().is_none());
            }
            "inherited_group" => {
                let filter = Filter::parse(&input["raw"], input["group"].as_str()).unwrap();
                assert_eq!(json!(filter.events()), case["native"]["events"]);
                assert_eq!(filter.source(), Source::File);
            }
            _ => panic!("unrecognized correction"),
        }
    }
}
