use super::*;
use hagency_runtime::codex::session::{Observation, ObservationKind, ToolResult};
use std::path::Path;
#[path = "../../../hagency-execution/examples/codex_qualify/witness.rs"]
mod probe;

fn command_event(command: Value, directory: Value, complete: bool) -> Value {
    let timestamp = if complete {
        "completedAtMs"
    } else {
        "startedAtMs"
    };
    note(
        if complete {
            "item/completed"
        } else {
            "item/started"
        },
        json!({
            "threadId":"thread-one", "turnId":"turn-one", timestamp:10,
            "item":{"id":"command-one", "type":"commandExecution", "command":command,
                "cwd":directory, "status":if complete { "completed" } else { "inProgress" },
                "exitCode":if complete { json!(0) } else { Value::Null }}
        }),
    )
}
async fn observe(session: &mut Session, peer: &mut Peer, event: Value) -> Observation {
    let (result, ()) = tokio::join!(session.next_observed_update(), write(peer, event));
    result.unwrap().1
}

#[tokio::test]
async fn native_codex_command_witness_exact() {
    let command = "printf qualified > 'disposable-target'";
    let (mut session, mut peer) = running().await;
    for complete in [false, true] {
        let observation = observe(
            &mut session,
            &mut peer,
            command_event(json!(command), json!(cwd()), complete),
        )
        .await;
        assert!(session.matches_observation_source(observation.source()));
        let ObservationKind::Tool(tool) = observation.kind() else {
            panic!("actual scoped command observation")
        };
        assert!(tool.matches_command(command, Path::new(&cwd())));
        assert!(!tool.matches_command("printf unrelated", Path::new(&cwd())));
        assert!(!tool.matches_command(command, &std::env::temp_dir().join("other-workspace")));
        assert!(
            tool.result()
                == if complete {
                    ToolResult::Completed
                } else {
                    ToolResult::Pending
                }
        );
    }
    for field in ["command", "cwd"] {
        let (mut session, mut peer) = running().await;
        observe(
            &mut session,
            &mut peer,
            command_event(json!(command), json!(cwd()), false),
        )
        .await;
        let mut event = command_event(json!(command), json!(cwd()), true);
        event["params"]["item"][field] = if field == "command" {
            json!("unrelated command")
        } else {
            json!(
                std::env::temp_dir()
                    .join("other-workspace")
                    .to_str()
                    .unwrap()
            )
        };
        assert!(matches!(
            observe(&mut session, &mut peer, event).await.kind(),
            ObservationKind::Invalidated
        ));
    }
}

#[tokio::test]
async fn native_codex_command_witness_terminal() {
    let command = "printf qualified > 'disposable-target'";
    for field in ["command", "cwd", "same"] {
        let (mut session, mut peer) = running().await;
        for complete in [false, true] {
            observe(
                &mut session,
                &mut peer,
                command_event(json!(command), json!(cwd()), complete),
            )
            .await;
        }
        let mut terminal = end("completed");
        let mut item = command_event(json!(command), json!(cwd()), true)["params"]["item"].clone();
        if field != "same" {
            item[field] = json!("substituted");
        }
        terminal["params"]["turn"]["items"] = json!([item]);
        let observed = observe(&mut session, &mut peer, terminal).await;
        assert!(if field == "same" {
            matches!(observed.kind(), ObservationKind::TurnEnded(_))
        } else {
            matches!(observed.kind(), ObservationKind::Invalidated)
        });
    }
    for (peer_command, peer_cwd) in [
        (Value::Null, json!(cwd())),
        (json!(command), Value::Null),
        (json!(command), json!("relative")),
        (json!("x".repeat(8193)), json!(cwd())),
        (json!(command), json!("x".repeat(8193))),
        (json!("bad\ncommand"), json!(cwd())),
    ] {
        let (mut session, mut peer) = running().await;
        for complete in [false, true] {
            let observed = observe(
                &mut session,
                &mut peer,
                command_event(peer_command.clone(), peer_cwd.clone(), complete),
            )
            .await;
            let ObservationKind::Tool(tool) = observed.kind() else {
                panic!("generic tool is not command proof")
            };
            assert!(!tool.matches_command(command, Path::new(&cwd())));
        }
    }
}

#[tokio::test]
async fn native_codex_command_probe_witness() {
    let directory = cwd();
    let target = Path::new(&directory).join("disposable'quoted-target");
    for case in [
        "completed",
        "failed",
        "unrelated",
        "repeated",
        "invalidated",
        "explanation",
    ] {
        let (mut session, mut peer) = running().await;
        let mut probe = probe::Probe::new(Path::new(&directory), &target);
        let command = probe.command().to_owned();
        assert!(command.contains("'\\''"));
        if case != "explanation" {
            for complete in [false, true] {
                let mut event = command_event(
                    json!(if case == "unrelated" {
                        "echo unrelated"
                    } else {
                        &command
                    }),
                    json!(directory),
                    complete,
                );
                if case == "failed" && complete {
                    event["params"]["item"]["status"] = json!("failed");
                    event["params"]["item"]["exitCode"] = json!(1);
                }
                if case == "invalidated" && complete {
                    event["params"]["item"]["command"] = json!("substituted");
                }
                probe.observe(observe(&mut session, &mut peer, event).await.kind());
            }
            if case == "repeated" {
                let mut event = command_event(json!(command), json!(directory), false);
                event["params"]["item"]["id"] = json!("command-two");
                probe.observe(observe(&mut session, &mut peer, event).await.kind());
            }
        }
        let witness = probe.witness();
        let valid = matches!(case, "completed" | "failed");
        assert_eq!(witness.valid, valid);
        assert_eq!(witness.attempted, valid);
        assert_eq!(witness.completed, case == "completed");
        assert_eq!(witness.failed, case == "failed");
        assert!(!witness.approval);
        assert_eq!(
            witness.observed_commands,
            if case == "explanation" {
                0
            } else if case == "repeated" {
                2
            } else {
                1
            }
        );
        assert_eq!(witness.observed_other_tools, 0);
    }
    for case in [
        "matching",
        "unrelated",
        "other_cwd",
        "network",
        "file",
        "wrong_scope",
    ] {
        let (mut session, mut peer) = running().await;
        session.enable_approvals().unwrap();
        let mut probe = probe::Probe::new(Path::new(&directory), &target);
        let mut event = approval_event(json!(7));
        event["params"]["command"] = json!(probe.command());
        if case == "unrelated" {
            event["params"]["command"] = json!("echo unrelated");
        }
        if case == "other_cwd" {
            event["params"]["cwd"] = json!(std::env::temp_dir().join("other"));
        }
        if case == "wrong_scope" {
            event["params"]["turnId"] = json!("other-turn");
        }
        if case == "network" {
            event["params"].as_object_mut().unwrap().remove("command");
            event["params"].as_object_mut().unwrap().remove("cwd");
            event["params"]["networkApprovalContext"] =
                json!({"host":"example.test", "protocol":"https"});
        }
        if case == "file" {
            event["method"] = json!("item/fileChange/requestApproval");
            event["params"].as_object_mut().unwrap().remove("command");
            event["params"].as_object_mut().unwrap().remove("cwd");
        }
        let (result, ()) = tokio::join!(session.next_observed_update(), write(&mut peer, event));
        if case == "wrong_scope" {
            assert!(matches!(result, Err(Error::Scope)));
            continue;
        }
        let (Update::Approval(request), _) = result.unwrap() else {
            panic!("scoped original callback")
        };
        probe.observe_approval(&request);
        let witness = probe.witness();
        assert_eq!(witness.valid, case == "matching");
        assert_eq!(witness.approval, case == "matching");
        assert!(!witness.attempted && !witness.completed && !witness.failed);
        assert_eq!(witness.observed_commands, 0);
        assert_eq!(witness.observed_other_tools, 0);
        // No response is prepared or sent: the original callback stays ungranted.
    }
}
