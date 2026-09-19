mod common;
use common::*;
use hagency_store::{DomainRepository, EffectOutcome, EffectState};

#[test]
fn native_provisioning_active_account_scope() {
    for case in [
        "exact",
        "fence",
        "payload",
        "ack",
        "registration",
        "revoked",
        "unknown",
        "failed",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.register(&registration()).unwrap();
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("active_sdk", "Worker", &pool, 100));
        db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let original = db.claim_effect().unwrap().unwrap();
        db.validate_provision_account(&original, &registration())
            .unwrap();
        assert!(
            db.validate_active_provision_account(&original, &registration())
                .is_err()
        );
        let outcome = match case {
            "unknown" => EffectOutcome::Unknown,
            "failed" => EffectOutcome::NotApplied {
                receipt: "explicit offline failed fixture".into(),
            },
            _ => EffectOutcome::Applied {
                receipt: "explicit offline activation, not physical factory proof".into(),
            },
        };
        db.observe_effect(&original.id, original.fence, &outcome)
            .unwrap();
        let mut supplied = original.clone();
        let mut reg = registration();
        match case {
            "fence" => supplied.fence += 1,
            "payload" => supplied.payload["resource"]["model"] = "foreign".into(),
            "ack" => supplied.state = EffectState::Complete,
            "registration" => {
                reg.generation += 1;
                db.register(&reg).unwrap();
            }
            "revoked" => {
                db.revoke("revoke", &original.engagement_id).unwrap();
            }
            _ => {}
        }
        assert_eq!(
            db.validate_active_provision_account(&supplied, &reg)
                .is_ok(),
            case == "exact",
            "{case}"
        );
        assert!(
            db.validate_provision_account(&original, &registration())
                .is_err()
        );
        let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        assert_eq!(
            sql.query_row("SELECT COUNT(*) FROM matrix_transports", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            sql.query_row("SELECT COUNT(*) FROM matrix_session_routes", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            sql.query_row("SELECT COUNT(*) FROM canonical_tasks", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
}
