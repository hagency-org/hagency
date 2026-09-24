//! Local protocol fixture only; never accepts production credentials.
use hagency_core::custody::Lane;
use hagency_palpo::{Adapter, CancellationToken, HostConfig, Limits, Step};
use hagency_store::{
    Repository, Store,
    outbound::{Command, RegistrationIdentity, Reply},
};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FLEET: &str = "hf_0123456789abcdef0123456789abcdef";
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[tokio::main]
async fn main() {
    let endpoint = std::env::args().nth(1).expect("local fixture endpoint");
    let parsed = reqwest::Url::parse(&endpoint).unwrap();
    assert_eq!(parsed.scheme(), "http");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    let dir = tempfile::tempdir().unwrap();
    let store = Store::start(Repository::open(&dir.path().join("private")).unwrap(), 16).unwrap();
    let config = HostConfig::new(
        RegistrationIdentity {
            binding: "reference-fixture".into(),
            registration_generation: 7,
            side_id: "example.test".into(),
            fleet_id: FLEET.into(),
            registration_fingerprint: "a".repeat(64),
        },
        &endpoint,
        "synthetic-machine-token-DO-NOT-USE",
        31,
        Limits {
            poll_wait: Duration::ZERO,
            ..Limits::default()
        },
    )
    .unwrap();
    let adapter = Adapter::attach(config, store.clone()).await.unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        adapter.poll_once(Lane::Matrix, &cancel).await.unwrap(),
        Step::Acknowledged
    );
    assert_eq!(
        adapter.poll_once(Lane::Work, &cancel).await.unwrap(),
        Step::Acknowledged
    );
    for (lane, id) in [
        (Lane::Matrix, "reference-matrix"),
        (Lane::Work, "reference-work"),
    ] {
        let Reply::Claim(Some(ticket)) = store
            .outbound(
                Command::Claim {
                    scope: adapter.scope(),
                    lane,
                    id: id.into(),
                    lease_ms: 30000,
                },
                now(),
            )
            .await
            .unwrap()
        else {
            panic!("claim");
        };
        let Reply::Started(work) = store.outbound(Command::Start(ticket), now()).await.unwrap()
        else {
            panic!("start");
        };
        assert_eq!(work.origin_machine_generation, 31);
        if lane == Lane::Matrix {
            assert_eq!(work.payload["body"]["extension"]["fraction"], 0.125);
            assert_eq!(work.payload["body"]["ephemeral"][0]["type"], "m.typing");
        }
        store
            .outbound(
                Command::Complete {
                    ticket: work.ticket,
                    result: json!({"fixtureReceipt":id}),
                },
                now(),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        adapter.poll_once(Lane::Matrix, &cancel).await.unwrap(),
        Step::Empty
    );
    adapter.freeze_update(json!({"heartbeat":true,"statuses":[{
        "v":1,"fleetId":FLEET,"requestId":"r1","state":"pending","role":"coding","requestedTokens":100000,
        "targetProjectId":"p1","targetRoomId":"!target:example.test","sourceRoomId":"!reception:example.test",
        "sourceEventId":"$request","observedAt":"2020-01-01T00:00:00.000Z"
    }]})).await.unwrap();
    assert_eq!(
        adapter.publish_once(&cancel).await.unwrap(),
        Step::Published
    );
    store.shutdown().await.unwrap();
    println!(
        "native client: Matrix/work ACK, preserved payload, empty poll and frozen status accepted"
    );
}
