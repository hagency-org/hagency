use super::*;
fn hook(method: &str, turn: bool) -> Value {
    note(
        method,
        json!({"threadId":"thread-one","turnId":if turn {Some("turn-one")} else {None},
        "run":{"id":"hook-one","eventName":if turn {"userPromptSubmit"} else {"sessionStart"},
            "executionMode":"sync","handlerType":"command","scope":if turn {"turn"} else {"thread"},
            "sourcePath":std::env::temp_dir().join("offline-hooks.json"),"source":"user","startedAt":1,"displayOrder":0,
            "status":if method=="hook/started" {"running"} else {"completed"},"entries":[]}}),
    )
}
#[tokio::test]
async fn native_codex_session_hook_notices() {
    let (mut session, mut peer) = fixture(false);
    initialize(&mut session, &mut peer).await;
    let (opened, _) = tokio::join!(
        session.start_thread(),
        exchange(
            &mut peer,
            thread_result(false),
            vec![hook("hook/started", false), hook("hook/completed", false)]
        )
    );
    assert_eq!(opened.unwrap(), "thread-one");
    assert_eq!(session.phase(), Phase::ThreadReady);
    start(&mut session, &mut peer, vec![hook("hook/started", true)])
        .await
        .unwrap();
    assert!(matches!(
        session.next_update().await.unwrap(),
        Update::Notice
    ));
    assert!(matches!(
        update(&mut session, &mut peer, hook("hook/completed", true))
            .await
            .unwrap(),
        Update::Notice
    ));
    assert!(session.outcome().is_none());
    assert_eq!(session.item_count(), 0);
    for kind in [
        "thread",
        "turn",
        "missing_run",
        "invalid_status",
        "oversized",
        "async",
        "private_label",
    ] {
        let (mut session, mut peer) = running().await;
        let mut event = hook("hook/started", true);
        let expected = match kind {
            "thread" => {
                event["params"]["threadId"] = "other".into();
                Error::Scope
            }
            "turn" => {
                event["params"]["turnId"] = "other".into();
                Error::Scope
            }
            "missing_run" => {
                event["params"]["run"] = Value::Null;
                Error::Malformed
            }
            "invalid_status" => {
                event["params"]["run"]["status"] = "completed".into();
                Error::Malformed
            }
            "oversized" => {
                event["params"]["run"]["entries"] =
                    json!([{"kind":"context","text":"x".repeat(65537)}]);
                Error::Capacity
            }
            "async" => {
                event["params"]["run"]["executionMode"] = "async".into();
                Error::Malformed
            }
            _ => {
                event["method"] = "private-peer-text".into();
                Error::UnsupportedEvent
            }
        };
        assert_eq!(
            update(&mut session, &mut peer, event).await.err(),
            Some(expected),
            "{kind}"
        );
        unknown(&session, expected);
        assert_eq!(
            session.refused_notification(),
            Some(if kind == "private_label" {
                "unknown"
            } else {
                "hook"
            })
        );
    }
    let (mut session, mut peer) = fixture(false);
    initialize(&mut session, &mut peer).await;
    let (opened, _) = tokio::join!(
        session.start_thread(),
        exchange(
            &mut peer,
            thread_result(false),
            vec![hook("hook/started", true)]
        )
    );
    assert_eq!(opened.err(), Some(Error::Scope));
}
