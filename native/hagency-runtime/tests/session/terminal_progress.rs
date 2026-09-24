use super::*;
use hagency_runtime::codex::session::ObservationKind;

fn command(complete: bool) -> Value {
    note(
        if complete {
            "item/completed"
        } else {
            "item/started"
        },
        json!({
            "threadId":"thread-one","turnId":"turn-one",
            if complete {"completedAtMs"} else {"startedAtMs"}:10,
            "item":{"id":"command-one","type":"commandExecution","command":"sleep 20","cwd":cwd(),
                "status":if complete {"completed"} else {"inProgress"},"exitCode":if complete {Some(0)} else {None}}
        }),
    )
}
fn terminal(stdin: Value) -> Value {
    note(
        "item/commandExecution/terminalInteraction",
        json!({
            "threadId":"thread-one","turnId":"turn-one","itemId":"command-one","processId":"321","stdin":stdin
        }),
    )
}

#[tokio::test]
async fn native_codex_session_terminal_progress() {
    let (mut session, mut peer) = running().await;
    update(&mut session, &mut peer, command(false))
        .await
        .unwrap();
    for stdin in ["", "input\n\u{3}"] {
        let (result, ()) = tokio::join!(
            session.next_observed_update(),
            write(&mut peer, terminal(json!(stdin)))
        );
        let (update, observation) = result.unwrap();
        assert!(matches!(update, Update::Progress));
        assert!(matches!(observation.kind(), ObservationKind::Ignored));
        assert_eq!(session.phase(), Phase::Running);
        assert!(session.outcome().is_none());
        assert_eq!(session.item_count(), 1);
        assert_eq!(session.text_bytes(), 0);
    }
    update(&mut session, &mut peer, command(true))
        .await
        .unwrap();
    assert!(matches!(
        update(&mut session, &mut peer, end("completed"))
            .await
            .unwrap(),
        Update::TurnEnded
    ));

    for case in [
        "thread",
        "turn",
        "item",
        "absent",
        "completed",
        "wrong_kind",
        "process",
        "stdin",
        "large",
        "unknown",
    ] {
        let (mut session, mut peer) = running().await;
        if case != "absent" {
            let mut item = command(false);
            if case == "wrong_kind" {
                item["params"]["item"]["type"] = "reasoning".into();
            }
            update(&mut session, &mut peer, item).await.unwrap();
            if case == "completed" {
                update(&mut session, &mut peer, command(true))
                    .await
                    .unwrap();
            }
        }
        let mut event = terminal(json!(""));
        let expected = match case {
            "thread" => {
                event["params"]["threadId"] = "other".into();
                Error::Scope
            }
            "turn" => {
                event["params"]["turnId"] = "other".into();
                Error::Scope
            }
            "item" => {
                event["params"]["itemId"] = "other".into();
                Error::Scope
            }
            "absent" | "completed" | "wrong_kind" => Error::Scope,
            "process" => {
                event["params"]["processId"] = json!(321);
                Error::Malformed
            }
            "stdin" => {
                event["params"]["stdin"] = Value::Null;
                Error::Malformed
            }
            "large" => {
                event["params"]["stdin"] = json!("x".repeat(MAX_TEXT_BYTES + 1));
                Error::Capacity
            }
            _ => {
                event["method"] = "private-peer-method".into();
                Error::UnsupportedEvent
            }
        };
        assert_eq!(
            update(&mut session, &mut peer, event).await.err(),
            Some(expected),
            "{case}"
        );
        unknown(&session, expected);
        assert_eq!(
            session.refused_notification(),
            Some(if case == "unknown" {
                "unknown"
            } else {
                "terminal_interaction"
            })
        );
    }
}
