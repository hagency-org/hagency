use super::*;
use hagency_store::{
    ApprovalResponseGrant, ApprovalResponseObservation as Observation,
    ApprovalResponseState as State,
};
use std::time::{Duration, Instant};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn grant(f: &mut Fixture, agent: usize, id: u64, choice: ApprovalChoice) -> ApprovalResponseGrant {
    let pending = f.admit(agent, id);
    if pending.choice.is_none() {
        f.choose(&pending.id, choice);
    }
    f.db.authorize_approval_response(&f.caps[agent], &pending.id, 1013)
        .unwrap()
}
fn mutation(
    f: &mut Fixture,
    agent: usize,
    id: &str,
) -> Result<hagency_core::tasks::MutationResult, Error> {
    f.db.mutate_task(
        &f.caps[agent],
        &format!("task_{}", if agent == 0 { "a" } else { "b" }),
        id,
        &TaskMutation::Comment {
            text: "bound task control".into(),
        },
        1015,
    )
}

#[test]
fn native_approval_router_authority() {
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: Clone> Ambiguous<u8> for T {}
    let _ = <ApprovalResponseGrant as Ambiguous<_>>::check;
    trait NoSerde<A> {
        fn check() {}
    }
    impl<T: ?Sized> NoSerde<()> for T {}
    impl<T: serde::de::DeserializeOwned> NoSerde<u8> for T {}
    let _ = <ApprovalResponseGrant as NoSerde<_>>::check;
    let mut f = Fixture::new(true);
    let a = f.admit(0, 1);
    let b = f.admit(0, 2);
    assert!(mutation(&mut f, 0, "parked").is_err());
    f.choose(&a.id, ApprovalChoice::Once);
    f.choose(&b.id, ApprovalChoice::Deny);
    let allow =
        f.db.authorize_approval_response(&f.caps[0], &a.id, 1013)
            .unwrap();
    let deny =
        f.db.authorize_approval_response(&f.caps[0], &b.id, 1013)
            .unwrap();
    assert!(allow.application().allow);
    assert!(!deny.application().allow);
    assert_eq!(f.state(0), "parked");
    assert!(f.db.park_dispatch(&f.caps[0], false, 1014).is_err());
    let mut batch = [allow, deny];
    f.db.begin_approval_responses(&f.caps[0], &mut batch, deadline(), 1014)
        .unwrap();
    assert!(batch.iter().all(ApprovalResponseGrant::is_admitted));
    assert_eq!(f.state(0), "started");
    mutation(&mut f, 0, "router_started").unwrap();
    for item in &batch {
        assert_eq!(
            f.db.approval_response_summary(&item.application().id)
                .unwrap()
                .application,
            ApplicationOutcome::Unknown
        );
    }
    // A new request reparks before these original responses have been written.
    // Its own authorization does not rearm their already admitted grants.
    let pending = f.admit(0, 3);
    assert!(mutation(&mut f, 0, "new_park").is_err());
    assert!(
        f.db.check_approval_response(&f.caps[0], &batch[0], 1015)
            .is_err()
    );
    f.choose(&pending.id, ApprovalChoice::Once);
    let mut next = [f
        .db
        .authorize_approval_response(&f.caps[0], &pending.id, 1015)
        .unwrap()];
    f.db.begin_approval_responses(&f.caps[0], &mut next, deadline(), 1015)
        .unwrap();
    f.db.check_approval_response(&f.caps[0], &batch[0], 1015)
        .unwrap();
    for item in &mut batch {
        let observed =
            f.db.observe_approval_response(item, Observation::WriteAccepted)
                .unwrap();
        assert!(observed.write_accepted);
        assert!(!item.is_admitted());
        assert!(
            f.db.check_approval_response(&f.caps[0], item, 1015)
                .is_err()
        );
        // Repeating an exact historical observation does not admit another write.
        assert!(
            f.db.observe_approval_response(item, Observation::WriteAccepted)
                .unwrap()
                .write_accepted
        );
    }
    assert!(
        f.db.begin_approval_responses(&f.caps[0], &mut batch, deadline(), 1015)
            .is_err()
    );
}

#[test]
fn native_approval_response_scope() {
    for changed in [
        "missing",
        "foreign_cap",
        "foreign_context",
        "grant",
        "expiry",
        "lease",
        "room",
    ] {
        let mut f = Fixture::new(true);
        let a = grant(&mut f, 0, 1, ApprovalChoice::Always);
        let mut at = 1014;
        let mut cap = f.caps[0].clone();
        let mut batch = vec![a];
        match changed {
            "missing" => {
                let _other = grant(&mut f, 0, 2, ApprovalChoice::Once);
            }
            "foreign_cap" => cap = f.caps[1].clone(),
            "foreign_context" => {
                let mut c = f.contexts[0].clone();
                c.id = "other_context".into();
                f.db.bind_approval_context(&cap, &c, 1013).unwrap();
                let mut input = f.input(0, 2);
                input.context_id = c.id;
                let p = f.db.request_owner_approval(&cap, &input, 1013).unwrap();
                if p.choice.is_none() {
                    f.choose(&p.id, ApprovalChoice::Once);
                }
                batch.push(f.db.authorize_approval_response(&cap, &p.id, 1013).unwrap());
            }
            "grant" => f.db.revoke_approval_grant(&f.grants(0)[0].id).unwrap(),
            "expiry" => at = 12000,
            "lease" => at = 61006,
            "room" => {
                let mut room = f.rooms[0].clone();
                room.available = false;
                f.db.observe_approval_room(&room, 1014).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            f.db.begin_approval_responses(&cap, &mut batch, deadline(), at)
                .is_err(),
            "{changed}"
        );
        assert!(!batch[0].is_admitted());
        assert_eq!(f.state(0), "parked");
        assert!(
            f.db.begin_approval_responses(&f.caps[0], &mut batch, deadline(), 1015)
                .is_err()
        );
        assert!(f.db.park_dispatch(&f.caps[0], false, 1015).is_err());
    }
    // A grant from an independently opened physical original repository is foreign.
    let mut a = Fixture::new(true);
    let mut b = Fixture::new(true);
    let foreign = grant(&mut a, 0, 1, ApprovalChoice::Once);
    let _same_data = grant(&mut b, 0, 1, ApprovalChoice::Once);
    assert!(
        b.db.begin_approval_responses(&a.caps[0], &mut [foreign], deadline(), 1014)
            .is_err()
    );
}

#[test]
fn native_approval_response_observations() {
    let mut f = Fixture::new(true);
    let mut batch = [grant(&mut f, 0, 1, ApprovalChoice::Once)];
    let app = batch[0].application().clone();
    f.db.observe_approval_application(&observed(&app, ApplicationOutcome::Applied), 1014)
        .unwrap();
    assert_eq!(f.state(0), "parked");
    assert!(f.db.park_dispatch(&f.caps[0], false, 1014).is_err());
    f.db.begin_approval_responses(&f.caps[0], &mut batch, deadline(), 1014)
        .unwrap();
    f.db.observe_approval_response(&mut batch[0], Observation::WriteAccepted)
        .unwrap();
    let new = f.admit(0, 2);
    f.db.observe_approval_application(&observed(&app, ApplicationOutcome::Applied), 1015)
        .unwrap();
    assert_eq!(f.state(0), "parked");
    assert!(mutation(&mut f, 0, "still_parked").is_err());
    f.choose(&new.id, ApprovalChoice::Deny);
    let mut next = [f
        .db
        .authorize_approval_response(&f.caps[0], &new.id, 1015)
        .unwrap()];
    f.db.begin_approval_responses(&f.caps[0], &mut next, deadline(), 1015)
        .unwrap();
    let unknown =
        f.db.observe_approval_response(&mut next[0], Observation::OutcomeUnknown)
            .unwrap();
    assert_eq!(unknown.response, State::OutcomeUnknown);
    assert_eq!(f.state(0), "parked");
    f.db.observe_approval_application(
        &observed(next[0].application(), ApplicationOutcome::Applied),
        1016,
    )
    .unwrap();
    assert_eq!(f.state(0), "parked");
    assert!(
        f.db.check_approval_response(&f.caps[0], &next[0], 1016)
            .is_err()
    );
    // A later uncertain outcome does not erase an actually observed local write.
    let history =
        f.db.observe_approval_response(&mut batch[0], Observation::OutcomeUnknown)
            .unwrap();
    assert!(history.write_accepted);
    assert_eq!(history.response, State::OutcomeUnknown);
    // Even current-schema Applied evidence after lease expiry cannot revive it.
    f.db.observe_approval_application(&observed(&app, ApplicationOutcome::Applied), 61006)
        .unwrap();
    assert_eq!(f.state(0), "parked");
    assert!(f.db.park_dispatch(&f.caps[0], false, 61006).is_err());
    // Late evidence for a legacy descriptor does not resume either.
    let mut old = Fixture::new(true);
    let p = old.admit(0, 1);
    old.choose(&p.id, ApprovalChoice::Once);
    let app = old
        .db
        .consume_owner_approval(&old.caps[0], &p.id, 1013)
        .unwrap();
    old.db
        .observe_approval_application(&observed(&app, ApplicationOutcome::Applied), 1014)
        .unwrap();
    assert_eq!(old.state(0), "parked");
    assert!(old.db.approval_response_summary(&p.id).is_err());
    assert!(old.db.park_dispatch(&old.caps[0], false, 1014).is_err());
}

#[test]
fn native_approval_response_recovery() {
    let mut f = Fixture::new(true);
    let pending = f.admit(0, 1);
    f.choose(&pending.id, ApprovalChoice::Once);
    let legacy =
        f.db.consume_owner_approval(&f.caps[0], &pending.id, 1013)
            .unwrap();
    let sql = f.sql();
    let path = f.root.path().join("state");
    drop(f.db);
    // Reconstruct the real preceding schema by removing only schema22 additions.
    sql.execute_batch("DROP TABLE approval_responses; PRAGMA user_version=21;")
        .unwrap();
    let mut db = DomainRepository::open(&path).unwrap();
    assert_eq!(count(&sql, "approval_responses"), 0);
    assert_eq!(db.approval_summary(&pending.id).unwrap().state, "uncertain");
    db.observe_approval_application(&observed(&legacy, ApplicationOutcome::Applied), 1020)
        .unwrap();
    assert!(
        db.authorize_approval_response(&f.caps[0], &pending.id, 1020)
            .is_err()
    );
    assert!(db.park_dispatch(&f.caps[0], false, 1020).is_err());

    let mut f = Fixture::new(true);
    let mut batch = [grant(&mut f, 0, 1, ApprovalChoice::Once)];
    let id = batch[0].application().id.clone();
    f.db.begin_approval_responses(&f.caps[0], &mut batch, deadline(), 1014)
        .unwrap();
    let path = f.root.path().join("state");
    drop(f.db);
    let mut db = DomainRepository::open(&path).unwrap();
    assert_eq!(
        db.approval_response_summary(&id).unwrap().response,
        State::OutcomeUnknown
    );
    assert!(
        db.check_approval_response(&f.caps[0], &batch[0], 1015)
            .is_err()
    );
    assert!(
        db.begin_approval_responses(&f.caps[0], &mut batch, deadline(), 1015)
            .is_err()
    );
}
