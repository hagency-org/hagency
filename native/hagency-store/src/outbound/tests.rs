use super::*;
use crate::{Repository, Store};
use hagency_core::custody::Delivery;
use rusqlite::params;
use serde_json::json;

fn activation(generation: u64) -> Activation {
    Activation {
        registration: RegistrationIdentity {
            binding: "managed".into(),
            registration_generation: 7,
            side_id: "matrix.example.test".into(),
            fleet_id: "hf_fixture".into(),
            registration_fingerprint: "a".repeat(64),
        },
        machine_generation: generation,
        credential_fingerprint: format!("{generation:064x}"),
    }
}
fn activate(db: &mut Repository, generation: u64) -> TransportScope {
    match db
        .outbound(Command::Activate(activation(generation)), 1)
        .unwrap()
    {
        Reply::Scope(s) => s,
        _ => panic!("scope"),
    }
}
fn setup() -> (tempfile::TempDir, Repository, TransportScope) {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Repository::open(&dir.path().join("state")).unwrap();
    let s = activate(&mut db, 31);
    (dir, db, s)
}
fn delivery(id: &str, lane: Lane) -> LeasedDelivery {
    LeasedDelivery {
        machine_generation: 31,
        id: id.into(),
        lane,
        kind: if lane == Lane::Matrix {
            Kind::Transaction
        } else {
            Kind::Request
        },
        payload: json!({"id":id,"weight":0.125}),
        token: format!("lease-{id}"),
        expires_at_ms: 30000,
    }
}
fn poll(db: &mut Repository, s: &TransportScope, lane: Lane) -> PollTicket {
    match db
        .outbound(
            Command::BeginPoll {
                scope: s.clone(),
                lane,
            },
            1,
        )
        .unwrap()
    {
        Reply::Poll(p) => p,
        _ => panic!("poll"),
    }
}
fn receive(db: &mut Repository, s: &TransportScope, d: LeasedDelivery) -> Receipt {
    let p = poll(db, s, d.lane);
    match db
        .outbound(
            Command::Receive {
                poll: p,
                delivery: d,
            },
            2,
        )
        .unwrap()
    {
        Reply::Received(r) => r,
        _ => panic!("receipt"),
    }
}
fn ack_ticket(db: &mut Repository, s: &TransportScope, lane: Lane, id: &str) -> AckTicket {
    match db
        .outbound(
            Command::BeginAck {
                scope: s.clone(),
                lane,
                id: id.into(),
            },
            3,
        )
        .unwrap()
    {
        Reply::AckTicket(t) => t,
        _ => panic!("ack"),
    }
}
fn ack(db: &mut Repository, s: &TransportScope, lane: Lane, id: &str) {
    let t = ack_ticket(db, s, lane, id);
    assert!(matches!(
        db.outbound(
            Command::Ack {
                ticket: t,
                response: AckResponse::Accepted
            },
            4
        )
        .unwrap(),
        Reply::Ack(AckResolution::Accepted)
    ));
}
fn view(db: &mut Repository, s: &TransportScope, lane: Lane, id: &str, now: u64) -> DeliveryView {
    match db
        .outbound(
            Command::View {
                scope: s.clone(),
                lane,
                id: id.into(),
            },
            now,
        )
        .unwrap()
    {
        Reply::View(v) => v,
        _ => panic!("view"),
    }
}
fn claim(
    db: &mut Repository,
    s: &TransportScope,
    lane: Lane,
    id: &str,
    now: u64,
) -> Option<ClaimTicket> {
    match db
        .outbound(
            Command::Claim {
                scope: s.clone(),
                lane,
                id: id.into(),
                lease_ms: 100,
            },
            now,
        )
        .unwrap()
    {
        Reply::Claim(c) => c,
        _ => panic!("claim"),
    }
}
fn start(db: &mut Repository, t: &ClaimTicket, now: u64) -> StartedWork {
    match db.outbound(Command::Start(t.clone()), now).unwrap() {
        Reply::Started(w) => w,
        _ => panic!("start"),
    }
}
fn complete(db: &mut Repository, t: &ClaimTicket, now: u64) -> String {
    match db
        .outbound(
            Command::Complete {
                ticket: t.clone(),
                result: json!({"adapterReceipt":"same-content","ratio":0.5}),
            },
            now,
        )
        .unwrap()
    {
        Reply::Finished { digest } => digest,
        _ => panic!("finish"),
    }
}
fn freeze(db: &mut Repository, s: &TransportScope, body: Value) -> PublicationTicket {
    match db
        .outbound(
            Command::FreezePublication {
                scope: s.clone(),
                body,
            },
            1,
        )
        .unwrap()
    {
        Reply::Publication(Some(t)) => t,
        _ => panic!("publication"),
    }
}
fn fixture_delivery() -> Delivery {
    Delivery {
        binding: "fixture".into(),
        generation: 1,
        id: "legacy".into(),
        lane: Lane::Matrix,
        kind: Kind::Transaction,
        payload: json!({"events":[]}),
    }
}

#[test]
fn native_outbound_custody_intake() {
    let (dir, mut db, s) = setup();
    let mut duplicate_binding = activation(31);
    duplicate_binding.registration.binding = "another-binding-for-same-fleet".into();
    assert!(matches!(
        db.outbound(Command::Activate(duplicate_binding), 1),
        Err(Error::Generation)
    ));
    let vectors: Value =
        serde_json::from_str(include_str!("../../../fixtures/outbound-custody.json")).unwrap();
    for vector in vectors["vectors"].as_array().unwrap() {
        assert_eq!(
            canonical::encode_transport(&vector["input"]).unwrap(),
            vector["canonical"]
        );
        assert_eq!(
            canonical::transport_digest(&vector["input"]).unwrap(),
            vector["sha256"]
        );
    }
    assert!(canonical::encode(&json!({"weight":0.25})).is_err());
    assert!(canonical::encode_payload(&json!({"__proto__":{}})).is_err());
    let mut forged = fixture_delivery();
    forged.binding = "managed".into();
    forged.generation = 7;
    assert!(matches!(db.receive(&forged, 1), Err(Error::Generation)));
    assert!(matches!(
        db.outbound(
            Command::BeginAck {
                scope: s.clone(),
                lane: Lane::Matrix,
                id: "same".into()
            },
            1
        ),
        Err(Error::NotFound)
    ));
    let old_poll = poll(&mut db, &s, Lane::Matrix);
    let current_poll = poll(&mut db, &s, Lane::Matrix);
    let mut d = delivery("same", Lane::Matrix);
    d.payload = vectors["vectors"][0]["input"]["payload"].clone();
    assert!(matches!(
        db.outbound(
            Command::Receive {
                poll: old_poll,
                delivery: d.clone()
            },
            2
        ),
        Err(Error::State)
    ));
    let mut wrong = d.clone();
    wrong.machine_generation = 7;
    assert!(matches!(
        db.outbound(
            Command::Receive {
                poll: current_poll.clone(),
                delivery: wrong
            },
            2
        ),
        Err(Error::Generation)
    ));
    let r = match db
        .outbound(
            Command::Receive {
                poll: current_poll.clone(),
                delivery: d.clone(),
            },
            2,
        )
        .unwrap()
    {
        Reply::Received(r) => r,
        _ => panic!("receipt"),
    };
    assert_eq!(r.generation, 7);
    assert!(
        matches!(db.outbound(Command::Receive{poll:current_poll.clone(),delivery:d.clone()},3).unwrap(),Reply::Received(retry) if retry==r)
    );
    // Another SQLite connection can see the full commit before any ACK begins.
    let read = rusqlite::Connection::open(dir.path().join("state/custody.sqlite3")).unwrap();
    let raw: String = read
        .query_row("SELECT payload FROM inbox WHERE id='same'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), d.payload);
    drop(read);
    let a = ack_ticket(&mut db, &s, Lane::Matrix, "same");
    let mut changed = d.clone();
    changed.token = "replacement".into();
    assert!(matches!(
        db.outbound(
            Command::Receive {
                poll: current_poll,
                delivery: changed.clone()
            },
            3
        ),
        Err(Error::Conflict)
    ));
    receive(&mut db, &s, changed.clone());
    assert!(matches!(
        db.outbound(
            Command::Ack {
                ticket: a,
                response: AckResponse::Accepted
            },
            4
        )
        .unwrap(),
        Reply::Ack(AckResolution::Replaced)
    ));
    let b = ack_ticket(&mut db, &s, Lane::Matrix, "same");
    assert!(matches!(
        db.outbound(
            Command::Ack {
                ticket: b,
                response: AckResponse::StaleLease
            },
            4
        )
        .unwrap(),
        Reply::Ack(AckResolution::Reclaim)
    ));
    receive(&mut db, &s, changed.clone());
    assert_eq!(
        view(&mut db, &s, Lane::Matrix, "same", 4).lease_state,
        "stale"
    );
    assert!(claim(&mut db, &s, Lane::Matrix, "blocked", 4).is_none());
    changed.token = "third-lease".into();
    receive(&mut db, &s, changed.clone());
    let c = ack_ticket(&mut db, &s, Lane::Matrix, "same");
    db.outbound(
        Command::Ack {
            ticket: c.clone(),
            response: AckResponse::Accepted,
        },
        4,
    )
    .unwrap();
    db.outbound(
        Command::Ack {
            ticket: c.clone(),
            response: AckResponse::Unknown,
        },
        4,
    )
    .unwrap();
    db.outbound(
        Command::Ack {
            ticket: c,
            response: AckResponse::StaleLease,
        },
        4,
    )
    .unwrap();
    assert_eq!(
        view(&mut db, &s, Lane::Matrix, "same", 4).lease_state,
        "accepted"
    );
    let public = serde_json::to_string(&view(&mut db, &s, Lane::Matrix, "same", 4)).unwrap();
    assert!(!public.contains("third-lease"));
    assert!(!public.contains("opaque"));
    assert!(!public.contains(&s.key));
    changed.payload["extra"] = json!(true);
    let p = poll(&mut db, &s, Lane::Matrix);
    assert!(matches!(
        db.outbound(
            Command::Receive {
                poll: p,
                delivery: changed
            },
            4
        ),
        Err(Error::Conflict)
    ));
    receive(&mut db, &s, delivery("same", Lane::Work)); // Lane is part of identity.
    let mut huge = delivery("huge", Lane::Work);
    huge.token = "\\".repeat(4096);
    let p = poll(&mut db, &s, Lane::Work);
    let command = Command::Receive {
        poll: p,
        delivery: huge.clone(),
    };
    assert!(
        command.input_bytes().unwrap() > serde_json::to_vec(&huge.payload).unwrap().len() + 8192
    );
    db.outbound(command, 4).unwrap();
    let t = ack_ticket(&mut db, &s, Lane::Work, "huge");
    let bytes = serde_json::to_vec(&t.wire_body()).unwrap().len();
    assert!(
        Command::Ack {
            ticket: t,
            response: AckResponse::Unknown
        }
        .input_bytes()
        .unwrap()
            >= bytes
    );
}

#[test]
fn native_outbound_custody_processing() {
    let (dir, mut db, s) = setup();
    for id in ["first", "second"] {
        receive(&mut db, &s, delivery(id, Lane::Matrix));
        ack(&mut db, &s, Lane::Matrix, id);
    }
    receive(&mut db, &s, delivery("request", Lane::Work));
    ack(&mut db, &s, Lane::Work, "request");
    let expired = claim(&mut db, &s, Lane::Matrix, "unstarted", 10).unwrap();
    assert!(matches!(
        db.outbound(Command::Start(expired), 110),
        Err(Error::State)
    ));
    let first = claim(&mut db, &s, Lane::Matrix, "started", 111).unwrap();
    let work = start(&mut db, &first, 112);
    assert_eq!(work.delivery.id, "first");
    assert_eq!(work.payload["weight"], 0.125);
    assert!(matches!(
        db.outbound(Command::Start(first.clone()), 113),
        Err(Error::State)
    ));
    assert!(claim(&mut db, &s, Lane::Matrix, "must-not-skip", 113).is_none());
    let request = claim(&mut db, &s, Lane::Work, "request-attempt", 113).unwrap();
    start(&mut db, &request, 114);
    complete(&mut db, &request, 115);
    drop(db);
    let mut db = Repository::open(&dir.path().join("state")).unwrap();
    let s = activate(&mut db, 31);
    let head = match db
        .outbound(
            Command::Head {
                scope: s.clone(),
                lane: Lane::Matrix,
            },
            116,
        )
        .unwrap()
    {
        Reply::Head(Some(h)) => h,
        _ => panic!("head"),
    };
    assert_eq!(head.processing_state, "unknown");
    assert_eq!(head.attempt_id.as_deref(), Some("started"));
    assert!(claim(&mut db, &s, Lane::Matrix, "blocked-after-restart", 116).is_none());
    assert!(matches!(
        db.outbound(
            Command::Complete {
                ticket: first.clone(),
                result: json!({"late":true})
            },
            116
        ),
        Err(Error::State)
    ));
    db.outbound(
        Command::Inspect {
            scope: s.clone(),
            attempt_id: first.id().into(),
            outcome: Inspection::Retry,
        },
        116,
    )
    .unwrap();
    db.outbound(
        Command::Inspect {
            scope: s.clone(),
            attempt_id: first.id().into(),
            outcome: Inspection::Retry,
        },
        116,
    )
    .unwrap();
    let retry = claim(&mut db, &s, Lane::Matrix, "recovery", 117).unwrap();
    start(&mut db, &retry, 118);
    let result = complete(&mut db, &retry, 119);
    assert_eq!(complete(&mut db, &retry, 120), result);
    assert!(matches!(
        db.outbound(
            Command::Complete {
                ticket: retry,
                result: json!({"changed":true})
            },
            120
        ),
        Err(Error::Conflict)
    ));
    let done = view(&mut db, &s, Lane::Matrix, "first", 120);
    assert_eq!(done.result_digest.as_deref(), Some(result.as_str()));
    let tombstone: String = db
        .db
        .query_row("SELECT payload FROM inbox WHERE id='first'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(tombstone, "{}");
    receive(&mut db, &s, delivery("first", Lane::Matrix));
    ack(&mut db, &s, Lane::Matrix, "first");
    let second = claim(&mut db, &s, Lane::Matrix, "second-attempt", 121).unwrap();
    assert_eq!(start(&mut db, &second, 122).delivery.id, "second");
    assert_eq!(
        view(&mut db, &s, Lane::Matrix, "second", 221).processing_state,
        "unknown"
    );
    db.outbound(
        Command::Inspect {
            scope: s.clone(),
            attempt_id: second.id().into(),
            outcome: Inspection::Completed(json!({"inspected":"committed"})),
        },
        222,
    )
    .unwrap();
    db.outbound(
        Command::Inspect {
            scope: s.clone(),
            attempt_id: second.id().into(),
            outcome: Inspection::Completed(json!({"inspected":"committed"})),
        },
        223,
    )
    .unwrap();
    assert!(matches!(
        db.outbound(
            Command::Inspect {
                scope: s,
                attempt_id: second.id().into(),
                outcome: Inspection::Completed(json!({"inspected":"changed"}))
            },
            224
        ),
        Err(Error::Conflict)
    ));
}

#[test]
fn native_outbound_custody_rotation() {
    let (dir, mut db, old) = setup();
    for id in ["accepted", "uncertain", "never-acked"] {
        receive(&mut db, &old, delivery(id, Lane::Matrix));
    }
    ack(&mut db, &old, Lane::Matrix, "accepted");
    let uncertain = ack_ticket(&mut db, &old, Lane::Matrix, "uncertain");
    receive(&mut db, &old, delivery("request", Lane::Work));
    ack(&mut db, &old, Lane::Work, "request");
    let request = claim(&mut db, &old, Lane::Work, "request-started", 10).unwrap();
    start(&mut db, &request, 11);
    let mut probe = delivery("probe", Lane::Work);
    probe.kind = Kind::Probe;
    receive(&mut db, &old, probe);
    let publication = freeze(&mut db, &old, json!({"heartbeat":true,"probeReceipts":[]}));
    db.outbound(Command::BeginPublication(publication.clone()), 12)
        .unwrap();
    let current = activate(&mut db, 32);
    assert_eq!(current.consumer(), old.consumer());
    assert!(matches!(
        db.outbound(
            Command::Ack {
                ticket: uncertain,
                response: AckResponse::Accepted
            },
            13
        ),
        Err(Error::Generation)
    ));
    assert!(matches!(
        db.outbound(
            Command::Publication {
                ticket: publication,
                response: PublicationResponse::Accepted
            },
            13
        ),
        Err(Error::Generation)
    ));
    assert!(matches!(
        db.outbound(
            Command::BeginPoll {
                scope: old,
                lane: Lane::Work
            },
            13
        ),
        Err(Error::Generation)
    ));
    assert_eq!(
        view(&mut db, &current, Lane::Matrix, "accepted", 13).lease_state,
        "accepted"
    );
    for id in ["uncertain", "never-acked"] {
        let v = view(&mut db, &current, Lane::Matrix, id, 13);
        assert_eq!(v.lease_state, "retired");
        assert_eq!(v.origin_machine_generation, 31);
        assert_eq!(v.receipt.generation, 7);
    }
    assert_eq!(
        view(&mut db, &current, Lane::Work, "probe", 13).processing_state,
        "retired"
    );
    // Unchanged registration keeps already-started request ownership valid.
    complete(&mut db, &request, 13);
    for (n, id) in ["accepted", "uncertain", "never-acked"]
        .into_iter()
        .enumerate()
    {
        let t = claim(
            &mut db,
            &current,
            Lane::Matrix,
            &format!("retained-{n}"),
            14,
        )
        .unwrap();
        let work = start(&mut db, &t, 15);
        assert_eq!(work.delivery.id, id);
        assert_eq!(work.origin_machine_generation, 31);
        complete(&mut db, &t, 16);
    }
    let fresh = freeze(
        &mut db,
        &current,
        json!({"heartbeat":true,"probeReceipts":[]}),
    );
    assert_eq!(fresh.sequence(), 1);
    assert!(!fresh.wire_body().contains("old"));
    let mut replaced = activation(33);
    replaced.registration.registration_generation = 8;
    assert!(matches!(
        db.outbound(Command::Activate(replaced), 17),
        Err(Error::Generation)
    ));
    let mut changed = activation(32);
    changed.credential_fingerprint = "f".repeat(64);
    assert!(matches!(
        db.outbound(Command::Activate(changed), 17),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        db.outbound(Command::Activate(activation(31)), 17),
        Err(Error::Generation)
    ));
    drop(db);
    let mut db = Repository::open(&dir.path().join("state")).unwrap();
    let reopened = activate(&mut db, 32);
    assert_eq!(reopened.consumer(), current.consumer());
    let mut duplicate = delivery("accepted", Lane::Matrix);
    duplicate.machine_generation = 32;
    receive(&mut db, &reopened, duplicate.clone());
    ack(&mut db, &reopened, Lane::Matrix, "accepted");
    assert!(claim(&mut db, &reopened, Lane::Matrix, "no-repeat", 18).is_none());
    duplicate.payload["changed"] = json!(true);
    let p = poll(&mut db, &reopened, Lane::Matrix);
    assert!(matches!(
        db.outbound(
            Command::Receive {
                poll: p,
                delivery: duplicate
            },
            18
        ),
        Err(Error::Conflict)
    ));
}

#[test]
fn native_outbound_custody_publication() {
    let (dir, mut db, s) = setup();
    let body = json!({"heartbeat":true,"statuses":[{"requestId":"fixture","observedAt":"2026-09-09T01:02:03.000Z","ready":true}],"capabilities":{"weight":0.25}});
    let first = freeze(&mut db, &s, body.clone());
    assert_eq!(first.sequence(), 1);
    db.outbound(Command::BeginPublication(first.clone()), 2)
        .unwrap();
    db.outbound(
        Command::Publication {
            ticket: first.clone(),
            response: PublicationResponse::Unknown,
        },
        3,
    )
    .unwrap();
    let raw = first.wire_body().to_string();
    assert!(matches!(
        db.outbound(
            Command::FreezePublication {
                scope: s.clone(),
                body: json!({"heartbeat":true,"changed":true})
            },
            4
        ),
        Err(Error::Conflict)
    ));
    drop(db);
    let mut db = Repository::open(&dir.path().join("state")).unwrap();
    let s = activate(&mut db, 31);
    let pending = match db
        .outbound(Command::PendingPublication(s.clone()), 900_000)
        .unwrap()
    {
        Reply::Publication(Some(p)) => p,
        _ => panic!("frozen"),
    };
    assert_eq!(pending.wire_body(), raw);
    assert!(raw.contains("2026-09-09T01:02:03.000Z"));
    db.outbound(Command::BeginPublication(pending.clone()), 900_001)
        .unwrap();
    db.outbound(
        Command::Publication {
            ticket: pending.clone(),
            response: PublicationResponse::Rejected,
        },
        900_002,
    )
    .unwrap();
    assert_eq!(freeze(&mut db, &s, body.clone()).wire_body(), raw);
    db.outbound(
        Command::Publication {
            ticket: pending.clone(),
            response: PublicationResponse::Accepted,
        },
        900_003,
    )
    .unwrap();
    db.outbound(
        Command::Publication {
            ticket: pending.clone(),
            response: PublicationResponse::Accepted,
        },
        900_004,
    )
    .unwrap();
    let second = freeze(&mut db, &s, json!({"heartbeat":true}));
    assert_eq!(second.sequence(), 2);
    db.outbound(
        Command::Publication {
            ticket: pending,
            response: PublicationResponse::Unknown,
        },
        900_005,
    )
    .unwrap();
    assert_eq!(freeze(&mut db, &s, json!({"heartbeat":true})).sequence(), 2);
    let mut altered = second.clone();
    altered.body.push(' ');
    assert!(matches!(
        db.outbound(Command::BeginPublication(altered), 900_005),
        Err(Error::Conflict)
    ));
    for body in [
        json!({"heartbeat":true,"sequence":4}),
        json!({"heartbeat":false}),
        json!({"heartbeat":true,"statuses":vec![json!({});201]}),
        json!({"heartbeat":true,"probeReceipts":vec![json!({});11]}),
    ] {
        assert!(matches!(
            db.outbound(
                Command::FreezePublication {
                    scope: s.clone(),
                    body
                },
                900_005
            ),
            Err(Error::Invalid(_))
        ));
    }
}

#[test]
fn native_outbound_custody_storage() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    // Build an actual custody1 store, including a legacy fixture row; no old JS
    // or live state is imported. A collision late in migration rolls all DDL back.
    let old = crate::database::open(
        &state,
        crate::database::Schema {
            name: "custody.sqlite3",
            lock: "owner.lock",
            application_id: 0x48414731,
            version: 1,
            sql: include_str!("../schema.sql"),
            verify: &["SELECT payload FROM inbox LIMIT 0"],
            migrations: &[],
        },
    )
    .unwrap();
    let fixture = fixture_delivery();
    old.connection
        .execute("INSERT INTO bindings VALUES('fixture',1)", [])
        .unwrap();
    let receipt = Receipt {
        id: fixture.id.clone(),
        lane: fixture.lane,
        generation: 1,
        digest: fixture.content_digest().unwrap(),
        received_at_ms: 1,
        state: hagency_core::custody::CustodyState::Received,
    };
    old.connection
        .execute(
            "INSERT INTO inbox VALUES('fixture','matrix','legacy',1,?1,?2,?3)",
            params![
                receipt.digest,
                serde_json::to_string(&fixture.payload).unwrap(),
                serde_json::to_string(&receipt).unwrap()
            ],
        )
        .unwrap();
    old.connection
        .execute("CREATE TABLE outbound_attempts(collision INTEGER)", [])
        .unwrap();
    drop(old);
    assert!(Repository::open(&state).is_err());
    let raw = rusqlite::Connection::open(state.join("custody.sqlite3")).unwrap();
    assert_eq!(
        raw.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(raw.prepare("SELECT processing_state FROM inbox").is_err());
    assert!(raw.prepare("SELECT * FROM outbound_transports").is_err());
    raw.execute("DROP TABLE outbound_attempts", []).unwrap();
    drop(raw);
    let mut db = Repository::open(&state).unwrap();
    assert_eq!(db.receive(&fixture, 20).unwrap(), receipt);
    assert_eq!(
        db.db
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let mut adopt = activation(31);
    adopt.registration.binding = "fixture".into();
    adopt.registration.registration_generation = 1;
    assert!(matches!(
        db.outbound(Command::Activate(adopt), 20),
        Err(Error::Generation)
    ));
    let s = activate(&mut db, 31);
    let p = poll(&mut db, &s, Lane::Matrix);
    db.db.execute_batch("CREATE TRIGGER fail_received AFTER INSERT ON inbox BEGIN SELECT RAISE(ABORT,'injected disk failure'); END;").unwrap();
    assert!(
        db.outbound(
            Command::Receive {
                poll: p.clone(),
                delivery: delivery("rollback", Lane::Matrix)
            },
            20
        )
        .is_err()
    );
    assert_eq!(
        db.db
            .query_row(
                "SELECT COUNT(*) FROM inbox WHERE binding='managed'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert!(db.db.query_row("SELECT response_digest FROM outbound_polls WHERE binding='managed' AND lane='matrix'",[],|r|r.get::<_,Option<String>>(0)).unwrap().is_none());
    db.db.execute_batch("DROP TRIGGER fail_received").unwrap();
    db.outbound(
        Command::Receive {
            poll: p,
            delivery: delivery("rollback", Lane::Matrix),
        },
        20,
    )
    .unwrap();
    ack(&mut db, &s, Lane::Matrix, "rollback");
    let t = claim(&mut db, &s, Lane::Matrix, "complete-rollback", 21).unwrap();
    start(&mut db, &t, 22);
    db.db.execute_batch("CREATE TRIGGER fail_tombstone BEFORE UPDATE OF payload ON inbox BEGIN SELECT RAISE(ABORT,'injected compaction failure'); END;").unwrap();
    assert!(
        db.outbound(
            Command::Complete {
                ticket: t.clone(),
                result: json!({"result":"committed"})
            },
            23
        )
        .is_err()
    );
    assert_eq!(
        view(&mut db, &s, Lane::Matrix, "rollback", 23).processing_state,
        "started"
    );
    assert!(
        view(&mut db, &s, Lane::Matrix, "rollback", 23)
            .result_digest
            .is_none()
    );
    db.db.execute_batch("DROP TRIGGER fail_tombstone").unwrap();
    complete(&mut db, &t, 24);
    db.max_records = 2;
    let next = poll(&mut db, &s, Lane::Matrix);
    assert!(matches!(
        db.outbound(
            Command::Receive {
                poll: next,
                delivery: delivery("capacity", Lane::Matrix)
            },
            25
        ),
        Err(Error::Capacity)
    ));
    receive(&mut db, &s, delivery("rollback", Lane::Matrix)); // Completed identical retry still fits.
    db.max_records = 10;
    db.max_payload_bytes = 1;
    let next = poll(&mut db, &s, Lane::Work);
    assert!(matches!(
        db.outbound(
            Command::Receive {
                poll: next,
                delivery: delivery("bytes", Lane::Work)
            },
            25
        ),
        Err(Error::Capacity)
    ));
    assert_eq!(
        db.db
            .query_row("SELECT COUNT(*) FROM inbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert!(matches!(
        db.outbound(
            Command::FreezePublication {
                scope: s.clone(),
                body: json!({"heartbeat":true})
            },
            25
        ),
        Err(Error::Capacity)
    ));
    assert_eq!(
        db.db
            .query_row("SELECT sequence FROM outbound_transports", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.max_payload_bytes = 16 * 1024 * 1024;
    receive(&mut db, &s, delivery("attempt-capacity", Lane::Work));
    ack(&mut db, &s, Lane::Work, "attempt-capacity");
    db.max_attempts = 1;
    assert!(matches!(
        db.outbound(
            Command::Claim {
                scope: s.clone(),
                lane: Lane::Work,
                id: "second-attempt".into(),
                lease_ms: 100
            },
            25
        ),
        Err(Error::Capacity)
    ));
    assert_eq!(
        view(&mut db, &s, Lane::Work, "attempt-capacity", 25).processing_state,
        "pending"
    );
    // Failed rotation rolls back scope, retired lease state and publication fence.
    let publication = freeze(&mut db, &s, json!({"heartbeat":true}));
    db.db.execute_batch("CREATE TRIGGER fail_rotation BEFORE UPDATE OF machine_generation ON outbound_transports BEGIN SELECT RAISE(ABORT,'injected rotation failure'); END;").unwrap();
    assert!(db.outbound(Command::Activate(activation(32)), 26).is_err());
    assert_eq!(
        view(&mut db, &s, Lane::Work, "attempt-capacity", 26).lease_state,
        "accepted"
    );
    assert_eq!(
        freeze(&mut db, &s, json!({"heartbeat":true})).wire_body(),
        publication.wire_body()
    );
    db.db.execute_batch("DROP TRIGGER fail_rotation").unwrap();
    // Structural reopening checks inspect all schema2 projections.
    db.db
        .execute_batch("ALTER TABLE outbound_publications RENAME COLUMN digest TO corrupt_digest")
        .unwrap();
    drop(db);
    assert!(matches!(Repository::open(&state), Err(Error::Schema)));
}

#[tokio::test]
async fn native_outbound_custody_worker() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::start(Repository::open(&dir.path().join("state")).unwrap(), 16).unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let store = store.clone();
        tasks.spawn(async move {
            match store
                .outbound(Command::Activate(activation(31)), 1)
                .await
                .unwrap()
            {
                Reply::Scope(s) => s,
                _ => panic!("scope"),
            }
        });
    }
    let s = tasks.join_next().await.unwrap().unwrap();
    while let Some(result) = tasks.join_next().await {
        let other = result.unwrap();
        assert_eq!(other.consumer(), s.consumer());
        assert_eq!(other.key, s.key);
    }
    let p = match store
        .outbound(
            Command::BeginPoll {
                scope: s.clone(),
                lane: Lane::Matrix,
            },
            1,
        )
        .await
        .unwrap()
    {
        Reply::Poll(p) => p,
        _ => panic!("poll"),
    };
    let d = delivery("concurrent", Lane::Matrix);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let store = store.clone();
        let p = p.clone();
        let d = d.clone();
        tasks.spawn(async move {
            match store
                .outbound(
                    Command::Receive {
                        poll: p,
                        delivery: d,
                    },
                    2,
                )
                .await
                .unwrap()
            {
                Reply::Received(r) => r,
                _ => panic!("receipt"),
            }
        });
    }
    let first = tasks.join_next().await.unwrap().unwrap();
    while let Some(result) = tasks.join_next().await {
        assert_eq!(first, result.unwrap());
    }
    let ticket = match store
        .outbound(
            Command::BeginAck {
                scope: s.clone(),
                lane: Lane::Matrix,
                id: "concurrent".into(),
            },
            3,
        )
        .await
        .unwrap()
    {
        Reply::AckTicket(t) => t,
        _ => panic!("ack"),
    };
    store
        .outbound(
            Command::Ack {
                ticket,
                response: AckResponse::Accepted,
            },
            4,
        )
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    // Concurrency is checked inside one unexpired lease. The old 100ms lease
    // could legally expire between two real SQLite transactions on Windows.
    // This valid request lease changes no worker timeout or production policy.
    const CONCURRENT_LEASE_MS: u64 = 120_000;
    let claims_started = std::time::Instant::now();
    for id in ["claim-a", "claim-b"] {
        let store = store.clone();
        let s = s.clone();
        tasks.spawn(async move {
            matches!(
                store
                    .outbound(
                        Command::Claim {
                            scope: s,
                            lane: Lane::Matrix,
                            id: id.into(),
                            lease_ms: CONCURRENT_LEASE_MS
                        },
                        5
                    )
                    .await
                    .unwrap(),
                Reply::Claim(Some(_))
            )
        });
    }
    let mut claims = 0;
    while let Some(result) = tasks.join_next().await {
        claims += usize::from(result.unwrap());
    }
    assert!(
        claims_started.elapsed() < std::time::Duration::from_millis(CONCURRENT_LEASE_MS),
        "fixture exceeded its explicitly requested concurrency lease"
    );
    assert_eq!(claims, 1);
    store.shutdown().await.unwrap();
    let store = Store::start(Repository::open(&dir.path().join("state")).unwrap(), 1).unwrap();
    let head = store
        .outbound(
            Command::Head {
                scope: s,
                lane: Lane::Matrix,
            },
            6,
        )
        .await
        .unwrap();
    assert!(matches!(head,Reply::Head(Some(v)) if v.processing_state=="pending"));
    store.shutdown().await.unwrap();
}

#[test]
fn native_outbound_custody_rotation_probe_authority() {
    for state in ["claimed", "started", "completed"] {
        let (_dir, mut db, s) = setup();
        let mut d = delivery("old-probe", Lane::Work);
        d.kind = Kind::Probe;
        receive(&mut db, &s, d);
        ack(&mut db, &s, Lane::Work, "old-probe");
        let ticket = claim(&mut db, &s, Lane::Work, "probe-attempt", 10).unwrap();
        if state != "claimed" {
            start(&mut db, &ticket, 11);
        }
        let proof = json!({"received":true,"sourceEventId":"$old","challenge":"old"});
        if state == "completed" {
            db.outbound(
                Command::Complete {
                    ticket: ticket.clone(),
                    result: proof.clone(),
                },
                12,
            )
            .unwrap();
            freeze(
                &mut db,
                &s,
                json!({"heartbeat":true,"probeReceipts":[proof.clone()]}),
            );
        }
        let current = activate(&mut db, 32);
        assert!(matches!(
            db.outbound(
                Command::FreezePublication {
                    scope: current.clone(),
                    body: json!({"heartbeat":true,"probeReceipts":[proof.clone()]})
                },
                13
            ),
            Err(Error::Generation)
        ));
        if state != "completed" {
            assert!(matches!(
                db.outbound(
                    Command::Complete {
                        ticket: ticket.clone(),
                        result: proof.clone()
                    },
                    13
                ),
                Err(Error::State)
            ));
            assert!(matches!(
                db.outbound(Command::Start(ticket.clone()), 13),
                Err(Error::State)
            ));
            assert!(matches!(
                db.outbound(
                    Command::Inspect {
                        scope: current.clone(),
                        attempt_id: ticket.id().into(),
                        outcome: Inspection::Completed(proof)
                    },
                    13
                ),
                Err(Error::State)
            ));
        }
        let mut fresh = delivery("new-probe", Lane::Work);
        fresh.machine_generation = 32;
        fresh.kind = Kind::Probe;
        receive(&mut db, &current, fresh);
        ack(&mut db, &current, Lane::Work, "new-probe");
        let fresh = claim(&mut db, &current, Lane::Work, "new-probe-attempt", 14).unwrap();
        start(&mut db, &fresh, 15);
        let proof = json!({"received":true,"sourceEventId":"$current","challenge":"new"});
        db.outbound(
            Command::Complete {
                ticket: fresh,
                result: proof.clone(),
            },
            16,
        )
        .unwrap();
        let publication = freeze(
            &mut db,
            &current,
            json!({"heartbeat":true,"probeReceipts":[proof]}),
        );
        assert!(publication.wire_body().contains("$current"));
        assert!(!publication.wire_body().contains("$old"));
    }
}

#[test]
fn native_outbound_custody_processing_explicit_unknown() {
    let (_dir, mut db, s) = setup();
    receive(&mut db, &s, delivery("adapter", Lane::Matrix));
    ack(&mut db, &s, Lane::Matrix, "adapter");
    let ticket = claim(&mut db, &s, Lane::Matrix, "attempt", 10).unwrap();
    start(&mut db, &ticket, 11);
    db.outbound(Command::ProcessingUnknown(ticket.clone()), 12)
        .unwrap();
    db.outbound(Command::ProcessingUnknown(ticket.clone()), 13)
        .unwrap();
    assert_eq!(
        view(&mut db, &s, Lane::Matrix, "adapter", 13).processing_state,
        "unknown"
    );
    assert!(matches!(
        db.outbound(
            Command::Complete {
                ticket: ticket.clone(),
                result: json!({"late":true})
            },
            13
        ),
        Err(Error::State)
    ));
    assert!(claim(&mut db, &s, Lane::Matrix, "must-not-repeat", 13).is_none());
    db.outbound(
        Command::Inspect {
            scope: s,
            attempt_id: ticket.id().into(),
            outcome: Inspection::Completed(json!({"host":"inspected durable domain receipt"})),
        },
        14,
    )
    .unwrap();
}

#[tokio::test]
async fn native_outbound_custody_worker_execution_clock() {
    use std::time::{Duration, Instant};
    for started in [false, true] {
        let (_dir, mut db, s) = setup();
        receive(&mut db, &s, delivery("clock", Lane::Matrix));
        ack(&mut db, &s, Lane::Matrix, "clock");
        let ticket = claim(&mut db, &s, Lane::Matrix, "clock-attempt", 10).unwrap();
        if started {
            start(&mut db, &ticket, 11);
        }
        let store = Store::start(db, 16).unwrap();
        let release = store.pause_for_test().await;
        let command = if started {
            Command::Complete {
                ticket,
                result: json!({"stale":"completion"}),
            }
        } else {
            Command::Start(ticket)
        };
        // A controlled old monotonic anchor represents time spent waiting for
        // the paused writer. The same production submission method is exercised.
        let queued = store.outbound_at(command, 12, Instant::now() - Duration::from_secs(1));
        tokio::pin!(queued);
        tokio::select! {biased;
            _=&mut queued=>panic!("paused writer returned early"),
            _=tokio::task::yield_now()=>{}
        }
        assert_eq!(store.queue_remaining(), 15);
        release.send(()).unwrap();
        assert!(matches!(queued.await, Err(Error::State)));
        let result = store
            .outbound(
                Command::View {
                    scope: s,
                    lane: Lane::Matrix,
                    id: "clock".into(),
                },
                1013,
            )
            .await
            .unwrap();
        assert!(
            matches!(result,Reply::View(v) if v.processing_state==if started{"unknown"}else{"pending"})
        );
        store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_outbound_custody_worker_elapsed_lease() {
    let (_dir, mut db, scope) = setup();
    receive(&mut db, &scope, delivery("short-lease", Lane::Matrix));
    ack(&mut db, &scope, Lane::Matrix, "short-lease");
    let first = claim(&mut db, &scope, Lane::Matrix, "first", 5).unwrap();
    let store = Store::start(db, 16).unwrap();
    // Use the existing host submission-time seam to model 200ms already spent
    // in a queue. The real writer must expire the first 100ms lease before
    // deciding whether a fresh attempt may claim the same delivery.
    let second = store
        .outbound_at(
            Command::Claim {
                scope,
                lane: Lane::Matrix,
                id: "second".into(),
                lease_ms: 120_000,
            },
            5,
            std::time::Instant::now() - std::time::Duration::from_millis(200),
        )
        .await
        .unwrap();
    let Reply::Claim(Some(second)) = second else {
        panic!("expired unstarted work must be reclaimable");
    };
    assert_ne!(first.id(), second.id());
    assert!(matches!(
        store.outbound(Command::Start(first), 205).await,
        Err(Error::State)
    ));
    let started = store
        .outbound(Command::Start(second.clone()), 205)
        .await
        .unwrap();
    assert!(matches!(started, Reply::Started(_)));
    assert!(matches!(
        store.outbound(Command::Start(second), 206).await,
        Err(Error::State)
    ));
    store.shutdown().await.unwrap();
}
