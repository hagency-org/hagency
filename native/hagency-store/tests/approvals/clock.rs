use super::*;
use hagency_store::DomainStore;
use std::{future::Future, time::Duration};

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn fixture() -> Fixture {
    Fixture::new_at(true, now().saturating_sub(20), 60_000)
}

fn verdict(f: &Fixture, id: &str) -> OwnerVerdictObservation {
    let card = f.db.private_approval(id, now()).unwrap();
    OwnerVerdictObservation {
        request_id: id.into(),
        request_digest: card.digest,
        binding_generation: card.binding_generation,
        server_name: "example.test".into(),
        room_id: card.room_id,
        sender_mxid: card.owner_mxid,
        event_id: "$clock-verdict".into(),
        encrypted: true,
        choice: ApprovalChoice::Always,
    }
}

// This connection only holds the real file's write lock and reads evidence.
// It never edits authority rows. The production DomainStore retains its writer.
/// `None` means the contention window was not modelled by this attempt: the
/// fixture woke too late to take the lock before expiry, or held it for so
/// long that the operation's unchanged 100 ms SQLite busy timeout, not the
/// released lock, would decide the outcome. A hosted runner under load loses
/// either race a few times in a thousand; the caller rebuilds the scenario a
/// bounded number of times instead of judging an unmodelled window.
async fn after_lock<T>(
    sql: &mut rusqlite::Connection,
    deadline: u64,
    operation: impl Future<Output = Result<T, Error>>,
) -> Option<Result<T, Error>> {
    // Finish setup with a generous lifetime, then enter the real contention
    // window shortly before expiry. Only the lock wait must fit the unchanged
    // production 100 ms SQLite busy timeout; fixture setup need not fit it.
    tokio::time::sleep(Duration::from_millis(
        deadline.saturating_sub(now()).saturating_sub(60),
    ))
    .await;
    let lock = sql
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let taken = now();
    if taken + 15 >= deadline {
        eprintln!(
            "contention window not modelled: lock taken {} ms before expiry; rebuilding",
            deadline.saturating_sub(taken)
        );
        lock.commit().unwrap();
        return None;
    }
    tokio::pin!(operation);
    assert!(
        tokio::time::timeout(Duration::from_millis(5), &mut operation)
            .await
            .is_err(),
        "actual production operation must still be blocked by the original SQLite lock"
    );
    assert!(now() < deadline, "operation must enter before expiry");
    tokio::time::sleep(Duration::from_millis(deadline.saturating_sub(now()) + 5)).await;
    let held = now().saturating_sub(taken);
    lock.commit().unwrap();
    if held >= 90 {
        eprintln!("contention window not modelled: fixture held the lock {held} ms; rebuilding");
        return None;
    }
    Some(
        tokio::time::timeout(Duration::from_secs(2), operation)
            .await
            .expect("original writer must finish after contention is released"),
    )
}

#[tokio::test]
async fn native_approval_admission_clock_after_lock() {
    for action in ["bind", "request", "direct_verdict", "sdk_verdict"] {
        for attempt in 0..5 {
            let mut f = fixture();
            let mut sql = f.sql();
            let deadline = now() + 1000;
            let cap = f.caps[0].clone();
            let mut input = f.input(0, 1);
            input.expires_at = deadline;
            let mut context = f.contexts[0].clone();
            context.id.push_str("_fresh");
            let pending = if action.ends_with("verdict") {
                Some(f.db.request_owner_approval(&cap, &input, now()).unwrap())
            } else {
                None
            };
            let decision = pending.as_ref().map(|p| verdict(&f, &p.id));
            let sdk = pending.as_ref().map(|p| ApprovalVerdictInput {
                target: f.db.approval_intake_target(&p.id, now()).unwrap(),
                verdict: decision.clone().unwrap(),
                source_digest: "a".repeat(64),
            });
            if action == "bind" {
                // Legitimate lease renewal sets the short deadline; no SQL setter.
                let at = now();
                f.db.renew_dispatch(&cap, at, deadline - at).unwrap();
            }
            let contexts = count(&sql, "approval_contexts");
            let requests = count(&sql, "owner_approvals");
            let store = DomainStore::start(f.db, 16).unwrap();
            let operation = async {
                match action {
                    "bind" => store.bind_approval_context(cap, context).await,
                    "request" => store.request_owner_approval(cap, input).await.map(|_| ()),
                    "direct_verdict" => store
                        .observe_owner_verdict(decision.unwrap())
                        .await
                        .map(|_| ()),
                    "sdk_verdict" => store.admit_approval_verdict(sdk.unwrap()).await.map(|_| ()),
                    _ => unreachable!(),
                }
            };
            let Some(result) = after_lock(&mut sql, deadline, operation).await else {
                assert!(attempt < 4, "{action}: contention window never modelled");
                store.shutdown().await.unwrap();
                continue;
            };
            assert!(
                matches!(result, Err(Error::RunnerAuthority)),
                "{action} must reject authority expired during actual contention"
            );
            assert_eq!(count(&sql, "approval_contexts"), contexts);
            assert_eq!(count(&sql, "owner_approvals"), requests);
            assert_eq!(count(&sql, "approval_verdict_receipts"), 0);
            assert_eq!(count(&sql, "approval_grants"), 0);
            if let Some(pending) = pending {
                assert_eq!(
                    store.approval_summary(pending.id).await.unwrap().state,
                    "pending"
                );
            }
            store.shutdown().await.unwrap();
            break;
        }
    }
}

#[tokio::test]
async fn native_approval_consumption_clock_after_lock() {
    for action in ["decided_request", "pending_request", "capability"] {
        for attempt in 0..5 {
            let mut f = fixture();
            let mut sql = f.sql();
            let deadline = now() + 1000;
            let cap = f.caps[0].clone();
            let mut input = f.input(0, 1);
            input.expires_at = if action == "capability" {
                now() + 60_000
            } else {
                deadline
            };
            let pending = f.db.request_owner_approval(&cap, &input, now()).unwrap();
            if action != "pending_request" {
                f.db.observe_owner_verdict(&verdict(&f, &pending.id), now())
                    .unwrap();
            }
            if action == "capability" {
                let at = now();
                f.db.renew_dispatch(&cap, at, deadline - at).unwrap();
            }
            let store = DomainStore::start(f.db, 16).unwrap();
            let Some(result) = after_lock(
                &mut sql,
                deadline,
                store.consume_owner_approval(cap.clone(), pending.id.clone()),
            )
            .await
            else {
                assert!(attempt < 4, "{action}: contention window never modelled");
                store.shutdown().await.unwrap();
                continue;
            };
            if action == "capability" {
                assert!(matches!(result, Err(Error::RunnerAuthority)));
                assert_eq!(
                    store
                        .approval_summary(pending.id.clone())
                        .await
                        .unwrap()
                        .state,
                    "decided"
                );
                let descriptor: Option<String> = sql
                    .query_row(
                        "SELECT application FROM owner_approvals WHERE id=?1",
                        [&pending.id],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert!(descriptor.is_none());
            } else {
                let application = result.unwrap();
                assert!(
                    !application.allow,
                    "expired request cannot consume an allow"
                );
                assert_eq!(application.id, pending.id);
                assert_eq!(application.upstream_id, input.upstream_id);
                assert_eq!(application.connection_id, f.contexts[0].connection_id);
                assert_eq!(
                    store
                        .approval_summary(pending.id.clone())
                        .await
                        .unwrap()
                        .state,
                    "applying"
                );
            }
            // The row reached `applying` (asserted above): the design names the
            // settled-state word `already_consumed`, not the generic authority
            // refusal (spec: "the consume refuses already_consumed / not_consumable
            // instead of the generic authority word").
            assert!(matches!(
                store.consume_owner_approval(cap, pending.id).await,
                Err(Error::AlreadyConsumed)
            ));
            store.shutdown().await.unwrap();
            break;
        }
    }
}

#[tokio::test]
async fn native_approval_application_clock_after_lock() {
    for attempt in 0..5 {
        let mut f = fixture();
        let mut sql = f.sql();
        let cap = f.caps[0].clone();
        let mut input = f.input(0, 1);
        input.expires_at = now() + 60_000;
        let pending = f.db.request_owner_approval(&cap, &input, now()).unwrap();
        f.db.observe_owner_verdict(&verdict(&f, &pending.id), now())
            .unwrap();
        let application =
            f.db.consume_owner_approval(&cap, &pending.id, now())
                .unwrap();
        assert!(application.allow);
        let at = now();
        let deadline = at + 1000;
        f.db.renew_dispatch(&cap, at, 1000).unwrap();
        let store = DomainStore::start(f.db, 16).unwrap();
        // Explicit authenticated host-observation fixture, not a callback-resolution
        // assertion or proof that the offline runtime actually applied this response.
        let Some(result) = after_lock(
            &mut sql,
            deadline,
            store.observe_approval_application(observed(&application, ApplicationOutcome::Applied)),
        )
        .await
        else {
            assert!(attempt < 4, "contention window never modelled");
            store.shutdown().await.unwrap();
            continue;
        };
        let result = result.unwrap();
        assert_eq!(result.state, "applied");
        let state: String = sql
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id=?1",
                [&cap.dispatch_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            state, "parked",
            "historical evidence cannot resume expired execution"
        );
        assert!(store.consume_owner_approval(cap, pending.id).await.is_err());
        store.shutdown().await.unwrap();
        break;
    }
}
