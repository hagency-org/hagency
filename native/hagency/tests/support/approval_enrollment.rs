//! PC-C0b test support: the scripted second-identity enrollment of the
//! approval collector against the shared fake peer (no live homeserver).
//!
//! The composition's approval collector is lazy — its owner handle opens the
//! SDK root only when the first card is sent (`Owner::open_existing`), and
//! `open_existing` requires an ALREADY-ENROLLED root. Production never
//! enrolls the bot (D-ADR114 observe: the operator's own host act); this
//! module is the test-side script that performs that host act BEFORE the
//! composition launches, against the same fake peer and the same on-disk
//! state the composition then runs on:
//!
//! 1. write `approval.ca.pem` so the enrollment AND the composition's
//!    approval collector both accept the fake peer's TLS certificate
//!    (config.rs reads it when present; without it the send cannot connect);
//! 2. open the composition's own state directory as a `DomainStore` and
//!    build the approval bot's `HostApprovalConfig` (second identity, the
//!    token/key files `with_approval` wrote, SDK root `state/approval-sdk`,
//!    the DM room, the fresh-account anchor);
//! 3. drive `observe` (the DM room observation) and `enroll_fresh_account`
//!    (the key protocol) over the fake peer with the crypto fixture peer
//!    answering — the exact flow the matrix crate's delivery suite drives;
//! 4. shut the store down cleanly so the composition can take the lock.
//!
//! The enrolled SDK root and the room observation persist in the state
//! directory; the composition's collector opens both at send time. Nothing
//! here is referenced by any production path.

#[path = "../fixtures/matrix_crypto_peer.rs"]
pub mod crypto;

use crate::fixture::common;
use hagency_core::replies::{MatrixTransportObservation, RoomPrivacy};
use hagency_matrix::{
    ApprovalCollector, CancellationToken, HostApprovalConfig, HostConfig, HostIdentity, HostRoom,
};
use serde_json::{Value, json};
use std::path::Path;

pub const BOT: &str = "@approval:example.test";
pub const DEVICE: &str = "APPROVAL_DEVICE";
pub const ROOM: &str = "!private:example.test";
/// The approval bot's own token, distinct from the ordinary worker's so the
/// two legs of the shared fake peer are routable by Authorization header.
pub const APPROVAL_TOKEN: &str = "synthetic-approval-bot-token-not-real";

fn bot_who() -> Value {
    json!({"user_id":BOT,"device_id":DEVICE,"is_guest":false})
}

fn dm_state() -> Value {
    json!([
        {"type":"m.room.member","state_key":crypto::HUMAN,"content":{"membership":"join"}},
        {"type":"m.room.member","state_key":BOT,"content":{"membership":"join"}},
        {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
        {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}
    ])
}

/// Is this request the approval bot's leg? (Its token is its own.)
pub fn is_approval(request: &common::Request) -> bool {
    request.headers.get("authorization") == Some(&format!("Bearer {APPROVAL_TOKEN}"))
}

/// Route one APPROVAL-leg HTTP request: the room refresh
/// (whoami/state/sync), the key protocol, the outbound secret share and the
/// encrypted room event itself. Mirrors the matrix delivery suite's
/// `respond`, bound to the bot's token.
pub async fn respond(request: common::Request, peer: &mut crypto::Peer) {
    assert!(
        is_approval(&request),
        "approval-leg routing saw the ordinary token: {}",
        request.target
    );
    let body = if request.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&request.body).unwrap()
    };
    if request.target.ends_with("/whoami") {
        request.json(200, bot_who());
    } else if request.target.ends_with("/state") {
        request.json(200, dm_state());
    } else if request.target.contains("/sync?") {
        request.json(
            200,
            json!({"next_batch":"approval-empty-original","rooms":{"join":{ROOM:{"timeline":{"events":[],"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}}),
        );
    } else if request.method == "PUT" && request.target.contains("/sendToDevice/") {
        peer.share(body).await;
        request.json(200, json!({}));
    } else if request.method == "PUT" && request.target.contains("/send/") {
        // The delivery itself: the peer decrypts the megolm event and
        // records its content — the observation the wiring test asserts.
        peer.decrypt(body, ROOM.try_into().unwrap()).await;
        request.json(200, json!({"event_id":"$card_accepted"}));
    } else {
        let response = peer
            .protocol(&request.method, &request.target, &body)
            .await
            .expect("fixed original approval protocol");
        request.json(response.0, response.1);
    }
}

/// Drive one collector operation to completion while routing every HTTP
/// request it makes through the crypto peer (the delivery suite's `drive`).
async fn drive<T>(
    future: impl std::future::Future<Output = T>,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
) -> T {
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => return result,
            request = fake.next() => respond(request, peer).await,
        }
    }
}

/// The scripted host act: enroll the approval bot's second identity into
/// the state directory the composition will run on, and hand back the
/// fresh-account anchor (for the driver configuration's `peer_masters`) and
/// the crypto peer (whose key state answers the composition's send leg).
/// `engagement_id` is the composition's own engagement — the collector's
/// capacity check requires it admitted, which the bootstrap fixture's setup
/// already did.
pub async fn enroll(
    state_dir: &Path,
    fake: &mut common::Fake,
    engagement_id: &str,
) -> (String, crypto::Peer) {
    hagency_store::private::write_new(
        &state_dir.join("approval.ca.pem"),
        include_bytes!("../../../hagency-matrix/tests/fixtures/ca.pem"),
    )
    .unwrap();
    let repository = hagency_store::DomainRepository::open(state_dir).unwrap();
    let store = hagency_store::DomainStore::start(repository, 32).unwrap();
    let mut peer = crypto::Peer::for_sender(BOT, DEVICE).await;
    let config = HostConfig::new(
        HostIdentity {
            server_name: "example.test".into(),
            registration_fingerprint: "a".repeat(64),
            transport: MatrixTransportObservation {
                engagement_id: engagement_id.into(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: BOT.into(),
                device_id: DEVICE.into(),
            },
        },
        &fake.endpoint,
        APPROVAL_TOKEN,
        state_dir.join("approval-sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: ROOM.into(),
            generation: 1,
            privacy: RoomPrivacy::Direct {
                human_mxid: crypto::HUMAN.into(),
            },
        }],
        hagency_matrix::Limits::default(),
    )
    .unwrap()
    .with_root_pem(include_bytes!(
        "../../../hagency-matrix/tests/fixtures/ca.pem"
    ))
    .unwrap();
    let approval = HostApprovalConfig::new(config, vec![engagement_id.into()])
        .unwrap()
        .with_fresh_account_enrollment(vec![(crypto::HUMAN.into(), peer.anchor())])
        .unwrap();
    let collector = ApprovalCollector::new(approval, store.clone()).unwrap();
    // The DM room observation, then the fresh-account enrollment itself
    // (device upload, master/self/user signing, signature upload, claim,
    // secret share): the write/claim counts the delivery suite asserts.
    let _ = drive(
        collector.observe(&CancellationToken::new()),
        fake,
        &mut peer,
    )
    .await;
    drive(
        collector.enroll_fresh_account(&CancellationToken::new()),
        fake,
        &mut peer,
    )
    .await
    .unwrap();
    assert_eq!(peer.writes.len(), 5, "the scripted enrollment completed");
    assert_eq!(peer.claims, 1);
    let anchor = peer.anchor();
    collector.close().await.unwrap();
    // The close job fences every room candidate on the way out
    // (approval_intake.rs's `fence_approval_candidates`, `prior=None`) — the
    // collector's own lifecycle retires the room it just observed, leaving
    // `available=0` and `current_approval_bindings` empty. The scripted host
    // act therefore re-states the SAME positive observation at generation 2
    // AFTER the close (a fresh positive write the close path cannot fence):
    // the room returns to `available=1`, the binding's room_generation moves
    // with it, and the composition's `bind_context` finds a live binding.
    store
        .observe_approval_room(hagency_core::approvals::ApprovalRoomObservation {
            engagement_id: engagement_id.to_owned(),
            registration_generation: 1,
            generation: 2,
            room_id: ROOM.to_owned(),
            device_id: DEVICE.to_owned(),
            joined: [crypto::HUMAN.to_owned(), BOT.to_owned()].into(),
            invite_only: true,
            encrypted: true,
            available: true,
        })
        .await
        .unwrap();
    common::shutdown_domain(&store, "pc-c0b-enrollment").await;
    (anchor, peer)
}
