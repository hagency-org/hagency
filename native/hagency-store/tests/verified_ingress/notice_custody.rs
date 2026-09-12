use super::*;

fn state(f: &Fixture, id: &str) -> String {
    f.db.verified_notice_receipt(id).unwrap().state
}
fn active(f: &Fixture, task: &IntentResult) -> String {
    f.sql()
        .query_row(
            "SELECT state FROM task_intents WHERE task_id=?1",
            [&task.task_id],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn native_notice_custody_activation() {
    for direct in [false, true] {
        let mut f = Fixture::new(direct);
        let (_, sequence, task) = setup_task(&mut f);
        let claim =
            f.db.claim_verified_task_notice(1013, 1000)
                .unwrap()
                .unwrap();
        let id = &claim.claim.notice.id;
        let token = &claim.claim.token;
        assert!(
            f.db.deliver_verified_task_notice(id, token, &notice_delivery(&claim), 1014)
                .is_err()
        );
        assert!(
            f.db.fail_task_notice(id, token, "retry", false, 1014)
                .is_err()
        );
        assert!(f.db.retry_task_notice(id, 1014).is_err());
        assert!(
            f.db.begin_verified_task_notice_send(id, &"b".repeat(64), 1014)
                .is_err()
        );
        let sql = f.sql();
        sql.execute_batch("CREATE TRIGGER fail_send BEFORE UPDATE ON task_notices WHEN NEW.state='sending' BEGIN SELECT RAISE(ABORT,'fixture send-start rollback'); END;").unwrap();
        assert!(
            f.db.begin_verified_task_notice_send(id, token, 1014)
                .is_err()
        );
        assert_eq!(state(&f, id), "claimed");
        sql.execute_batch("DROP TRIGGER fail_send").unwrap();
        let send =
            f.db.begin_verified_task_notice_send(id, token, 1014)
                .unwrap();
        assert_eq!(send.fence, 1);
        assert_eq!(send.digest, claim.digest);
        assert_eq!(send.source_event_id, "$root");
        assert_eq!(send.notice.body, claim.claim.notice.body);
        assert_eq!(send.route.thread_root, claim.route.thread_root);
        assert!(
            f.db.begin_verified_task_notice_send(id, token, 1015)
                .is_err()
        );
        assert_eq!(active(&f, &task), "pending");
        f.db.deliver_verified_task_notice(id, token, &notice_delivery(&claim), 1016)
            .unwrap();
        assert_eq!(active(&f, &task), "active");
        assert!(
            f.db.deliver_verified_task_notice(id, token, &notice_delivery(&claim), 1017)
                .unwrap()
                .replayed
        );
        assert!(f.db.cancel_verified_task_notice(id, 1018).is_err());
        let cap = f.start("work", &task, &[sequence], 1019);
        assert_eq!(f.db.runner_inbox(&cap, 0, 100, 1021).unwrap().len(), 1);
    }
}

#[test]
fn native_notice_custody_recovery() {
    for restart in [false, true] {
        for started in [false, true] {
            let mut f = Fixture::new(true);
            let (_, _, task) = setup_task(&mut f);
            let old = f.db.claim_verified_task_notice(1013, 10).unwrap().unwrap();
            let id = &old.claim.notice.id;
            if started {
                f.db.begin_verified_task_notice_send(id, &old.claim.token, 1014)
                    .unwrap();
            }
            if restart {
                drop(f.db);
                f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
            }
            let next = f.db.claim_verified_task_notice(1030, 1000).unwrap();
            assert!(
                f.db.deliver_verified_task_notice(
                    id,
                    &old.claim.token,
                    &notice_delivery(&old),
                    1031
                )
                .is_err()
            );
            if !started {
                let next = next.unwrap();
                assert_ne!(next.claim.token, old.claim.token);
                assert_eq!(
                    next.claim.notice.transaction_id,
                    old.claim.notice.transaction_id
                );
                assert_eq!(
                    f.db.begin_verified_task_notice_send(id, &next.claim.token, 1031)
                        .unwrap()
                        .fence,
                    2
                );
                continue;
            }
            assert!(next.is_none());
            assert_eq!(state(&f, id), "uncertain");
            assert_eq!(active(&f, &task), "pending");
            let proof = ReplyReconciliation::NotSent {
                evidence: "fixture receiver and transaction inspection proved absence".into(),
            };
            assert!(
                f.db.reconcile_verified_task_notice(id, 2, &proof, 1031)
                    .is_err()
            );
            assert_eq!(
                f.db.reconcile_verified_task_notice(id, 1, &proof, 1031)
                    .unwrap()
                    .state,
                "pending"
            );
            assert!(
                f.db.reconcile_verified_task_notice(id, 1, &proof, 1032)
                    .unwrap()
                    .replayed
            );
            let wrong = ReplyReconciliation::NotSent {
                evidence: "changed evidence".into(),
            };
            assert!(matches!(
                f.db.reconcile_verified_task_notice(id, 1, &wrong, 1032),
                Err(Error::Conflict)
            ));
            let next = f.db.claim_verified_task_notice(1033, 10).unwrap().unwrap();
            let send =
                f.db.begin_verified_task_notice_send(id, &next.claim.token, 1034)
                    .unwrap();
            assert_eq!(send.fence, 2);
            assert!(
                f.db.claim_verified_task_notice(1044, 100)
                    .unwrap()
                    .is_none()
            );
            let observed = ReplyReconciliation::Delivered(notice_delivery(&next));
            let sql = f.sql();
            sql.execute_batch("CREATE TRIGGER fail_inspection BEFORE INSERT ON notice_send_inspections BEGIN SELECT RAISE(ABORT,'fixture inspection rollback'); END;").unwrap();
            assert!(
                f.db.reconcile_verified_task_notice(id, 2, &observed, 1045)
                    .is_err()
            );
            assert_eq!(state(&f, id), "uncertain");
            assert_eq!(active(&f, &task), "pending");
            sql.execute_batch("DROP TRIGGER fail_inspection").unwrap();
            f.db.reconcile_verified_task_notice(id, 2, &observed, 1046)
                .unwrap();
            assert_eq!(active(&f, &task), "active");
        }
    }
}

#[test]
fn native_notice_custody_fencing() {
    for reason in ["promotion", "revoke", "cancel"] {
        for delivered in [false, true] {
            let mut f = Fixture::new(true);
            let (_, _, task) = setup_task(&mut f);
            let claim =
                f.db.claim_verified_task_notice(1013, 1000)
                    .unwrap()
                    .unwrap();
            let id = &claim.claim.notice.id;
            f.db.begin_verified_task_notice_send(id, &claim.claim.token, 1014)
                .unwrap();
            match reason {
                "promotion" => {
                    f.room.generation = 2;
                    f.room.privacy = RoomPrivacy::Group {};
                    f.room.joined.insert("@third:example.test".into());
                    f.db.observe_matrix_room(&f.room, 1015).unwrap();
                }
                "revoke" => {
                    f.db.revoke("revoke", &f.agents[0]).unwrap();
                }
                _ => {
                    f.db.cancel_verified_task_notice(id, 1015).unwrap();
                }
            }
            assert!(
                f.db.begin_verified_task_notice_send(id, &claim.claim.token, 1016)
                    .is_err()
            );
            assert!(
                f.db.deliver_verified_task_notice(
                    id,
                    &claim.claim.token,
                    &notice_delivery(&claim),
                    1016
                )
                .is_err()
            );
            assert!(
                f.db.claim_verified_task_notice(1016, 1000)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(state(&f, id), "uncertain");
            let proof = if delivered {
                ReplyReconciliation::Delivered(notice_delivery(&claim))
            } else {
                ReplyReconciliation::NotSent {
                    evidence: "fixture observed no acceptance".into(),
                }
            };
            let result =
                f.db.reconcile_verified_task_notice(id, 1, &proof, 1017)
                    .unwrap();
            assert!(result.cancel_requested);
            assert_eq!(
                result.state,
                if delivered { "delivered" } else { "cancelled" }
            );
            assert_eq!(active(&f, &task), "pending");
            assert!(
                f.db.claim_verified_task_notice(1018, 1000)
                    .unwrap()
                    .is_none()
            );
            let projection = serde_json::to_string(&result).unwrap();
            assert!(
                !projection.contains("example.test") && !projection.contains(&claim.claim.token)
            );
        }
    }
    // A continuation notice cannot outlive the task epoch in which it was added.
    let mut f = Fixture::new(true);
    let (_, seq, task) = setup_task(&mut f);
    f.activate(1013);
    let cap = f.start("first", &task, &[seq], 1015);
    f.done(&cap, &task, 1018);
    f.db.complete_dispatch(&cap, &json!({"done":true}), 1019)
        .unwrap();
    let event = f.event("a", "followup", None, &[], 1020);
    let next = f.db.admit_matrix_event(&event, 1021).unwrap();
    let cap = f.start("followup", &task, &[next.sequence], 1022);
    let claim =
        f.db.claim_verified_task_notice(1024, 1000)
            .unwrap()
            .unwrap();
    f.done(&cap, &task, 1025);
    assert!(
        f.db.begin_verified_task_notice_send(&claim.claim.notice.id, &claim.claim.token, 1026)
            .is_err()
    );
    assert!(
        f.db.claim_verified_task_notice(1026, 1000)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&f, &claim.claim.notice.id), "cancelled");
}

#[test]
fn native_notice_custody_migration() {
    for claimed in [false, true] {
        let mut f = Fixture::new(true);
        let (_, _, task) = setup_task(&mut f);
        let claim = if claimed {
            Some(
                f.db.claim_verified_task_notice(1013, 1000)
                    .unwrap()
                    .unwrap(),
            )
        } else {
            None
        };
        let sql = f.sql();
        drop(f.db);
        common::remove_notice_schema(&sql);
        sql.pragma_update(None, "user_version", 13).unwrap();
        f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
        assert_eq!(
            sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            23
        );
        assert!(
            f.db.claim_verified_task_notice(1014, 1000)
                .unwrap()
                .is_none()
        );
        let result = f.db.verified_notice_receipt(&task.command_id).unwrap();
        assert_eq!(
            result.state,
            if claimed { "uncertain" } else { "cancelled" }
        );
        assert!(result.cancel_requested);
        if let Some(claim) = claim {
            assert!(
                f.db.begin_verified_task_notice_send(&task.command_id, &claim.claim.token, 1015)
                    .is_err()
            );
            f.db.reconcile_verified_task_notice(
                &task.command_id,
                1,
                &ReplyReconciliation::Delivered(notice_delivery(&claim)),
                1016,
            )
            .unwrap();
            assert_eq!(state(&f, &task.command_id), "delivered");
        }
        assert_eq!(active(&f, &task), "pending");
    }
}

#[test]
fn native_matrix_transport_negative_preserves_notice_custody() {
    for sending in [false, true] {
        let mut f = Fixture::new(true);
        let (_, _, task) = setup_task(&mut f);
        let claim =
            f.db.claim_verified_task_notice(1013, 1000)
                .unwrap()
                .unwrap();
        if sending {
            f.db.begin_verified_task_notice_send(&claim.claim.notice.id, &claim.claim.token, 1014)
                .unwrap();
        }
        let expected =
            f.db.matrix_transport_state(&f.agents[0])
                .unwrap()
                .unwrap()
                .observation;
        f.db.invalidate_matrix_transport(
            &hagency_core::replies::MatrixTransportInvalidation {
                expected,
                reason: "account failed".into(),
            },
            1015,
        )
        .unwrap();
        assert_eq!(
            state(&f, &claim.claim.notice.id),
            if sending { "uncertain" } else { "cancelled" }
        );
        assert_eq!(active(&f, &task), "pending");
        assert!(
            f.db.deliver_verified_task_notice(
                &claim.claim.notice.id,
                &claim.claim.token,
                &notice_delivery(&claim),
                1016
            )
            .is_err()
        );
    }
}
