use super::*;
use std::time::{Duration, Instant};

#[test]
fn native_owned_approval_maintenance() {
    let mut f = Fixture::configured(true, 1000, 60_000, false);
    let until = Instant::now() + Duration::from_secs(5);
    let scope =
        f.db.bind_owned_approval_context(
            &f.caps[0],
            &f.fingerprints[0],
            &f.contexts[0],
            until,
            6000,
            1010,
        )
        .unwrap();
    assert!(
        f.db.bind_owned_approval_context(
            &f.caps[0],
            &f.fingerprints[0],
            &f.contexts[0],
            until,
            6001,
            1011
        )
        .is_err()
    );
    let current = f.db.maintain_owned_approval(&scope, 1011).unwrap();
    assert!(!current.parked);
    let request = f.admit(0, 1);
    let current = f.db.maintain_owned_approval(&scope, 1012).unwrap();
    assert!(current.parked);
    assert_eq!(current.approvals.len(), 1);
    assert_eq!(current.approvals[0].id, request.id);
    assert!(
        f.db.mutate_task(
            &f.caps[0],
            "task_a",
            "parked_test",
            &TaskMutation::Comment {
                text: "parked mutation".into()
            },
            1013
        )
        .is_err()
    );
    assert!(
        f.db.check_owned_dispatch(&f.caps[0], &f.fingerprints[0], 1013)
            .is_err()
    );
    let mut other = Fixture::configured(true, 1000, 60_000, false);
    assert!(other.db.maintain_owned_approval(&scope, 1013).is_err());
    f.choose(&request.id, ApprovalChoice::Deny);
    let mut grants = vec![
        f.db.authorize_approval_response(&f.caps[0], &request.id, 1014)
            .unwrap(),
    ];
    f.db.begin_approval_responses(&f.caps[0], &mut grants, until, 1015)
        .unwrap();
    assert!(!f.db.maintain_owned_approval(&scope, 1016).unwrap().parked);
    f.db.observe_approval_response(
        &mut grants[0],
        hagency_store::ApprovalResponseObservation::WriteAccepted,
    )
    .unwrap();
    assert!(!f.db.maintain_owned_approval(&scope, 5999).unwrap().parked);
    assert!(f.db.maintain_owned_approval(&scope, 6000).is_err());
    let lease: u64 = f
        .sql()
        .query_row(
            "SELECT lease_until FROM runner_dispatches WHERE id='dispatch_a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lease, 6000);
    drop(f.db);
    let mut reopened = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert!(reopened.maintain_owned_approval(&scope, 1013).is_err());

    for changed in ["room", "fingerprint", "epoch", "retire", "lease"] {
        let mut f = Fixture::configured(true, 1000, 60_000, false);
        let until = Instant::now() + Duration::from_secs(5);
        let scope =
            f.db.bind_owned_approval_context(
                &f.caps[0],
                &f.fingerprints[0],
                &f.contexts[0],
                until,
                6000,
                1010,
            )
            .unwrap();
        f.admit(0, 1);
        match changed {
            "room" => {
                let mut room = f.rooms[0].clone();
                room.available = false;
                f.db.observe_approval_room(&room, 1012).unwrap();
            }
            "fingerprint" => {
                f.sql().execute("UPDATE runner_dispatches SET input=json_set(input,'$.payload.instruction','changed') WHERE id='dispatch_a'", []).unwrap();
            }
            "epoch" => {
                f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.execution_epoch',1) WHERE id='task_a'", []).unwrap();
            }
            "retire" => {
                f.db.revoke("owned_approval_release", &f.agents[0]).unwrap();
            }
            _ => {
                f.sql()
                    .execute(
                        "UPDATE runner_dispatches SET lease_until=1011 WHERE id='dispatch_a'",
                        [],
                    )
                    .unwrap();
            }
        }
        assert!(
            f.db.maintain_owned_approval(&scope, 1013).is_err(),
            "{changed}"
        );
    }
}
