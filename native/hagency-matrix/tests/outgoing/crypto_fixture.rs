//! Offline identities/sessions only. Production cannot call this trust bootstrap.
use super::Sdk;
use matrix_sdk_crypto::{
    DecryptionSettings, EncryptionSettings, EncryptionSyncChanges, OlmMachine, TrustRequirement,
    UserIdentity, types::requests::AnyOutgoingRequest,
};
use ruma::{api::client::keys::claim_keys, room_id, serde::Raw};
use serde_json::{Value, json};

pub(crate) struct Peer {
    pub query: Value,
    pub human: OlmMachine,
    pub old_session: String,
}
pub(super) async fn prepare(sdk: &Sdk, verified: bool) -> Peer {
    let (human, mut query) = super::crypto_fixture::verified_pair(sdk, verified).await;
    let guard = sdk.client.olm_machine().await;
    let sender = guard.as_ref().unwrap();
    // The human trusts the sender too, so actual received to-device/room data
    // can be decrypted with the same strict trust requirement as native intake.
    if let UserIdentity::Own(own) = human
        .get_identity(human.user_id(), None)
        .await
        .unwrap()
        .unwrap()
    {
        own.verify().await.unwrap();
    } else {
        panic!("own");
    }
    if let UserIdentity::Other(other) = human
        .get_identity(sender.user_id(), None)
        .await
        .unwrap()
        .unwrap()
    {
        let signed = other.verify().await.unwrap();
        query.master_keys.insert(
            sender.user_id().to_owned(),
            serde_json::from_str(
                signed.signed_keys[sender.user_id()]
                    .iter()
                    .next()
                    .unwrap()
                    .1
                    .get(),
            )
            .unwrap(),
        );
    } else {
        panic!("other");
    }
    let (id, _) = human.query_keys_for_users([sender.user_id(), human.user_id()]);
    human.mark_request_as_sent(&id, &query).await.unwrap();
    let outgoing = human.outgoing_requests().await.unwrap();
    let key = outgoing
        .iter()
        .find_map(|r| match r.request() {
            AnyOutgoingRequest::KeysUpload(req) => req.one_time_keys.iter().next(),
            _ => None,
        })
        .unwrap();
    let mut claim = claim_keys::v3::Response::new(Default::default());
    claim.one_time_keys.insert(
        human.user_id().to_owned(),
        [(
            human.device_id().to_owned(),
            [(key.0.clone(), key.1.clone())].into(),
        )]
        .into(),
    );
    let (id, _) = sender
        .get_missing_sessions([human.user_id()].into_iter())
        .await
        .unwrap()
        .unwrap();
    sender.mark_request_as_sent(&id, &claim).await.unwrap();
    // Seed an older room session. The actual adapter must force a fresh key,
    // even when current membership happens to be unchanged.
    sender
        .share_room_key(
            room_id!("!project:example.test"),
            [human.user_id()].into_iter(),
            EncryptionSettings::default(),
        )
        .await
        .unwrap();
    let old = sender
        .encrypt_room_event(
            room_id!("!project:example.test"),
            ruma::events::room::message::RoomMessageEventContent::text_plain("old fixture content"),
        )
        .await
        .unwrap();
    let old: Value = serde_json::to_value(old.content).unwrap();
    Peer {
        query: json!({"device_keys":query.device_keys,"master_keys":query.master_keys,"self_signing_keys":query.self_signing_keys,"user_signing_keys":query.user_signing_keys,"failures":{}}),
        human,
        old_session: old["session_id"].as_str().unwrap().into(),
    }
}
impl Peer {
    pub async fn share(&self, value: Value) {
        let content =
            &value["messages"][self.human.user_id().as_str()][self.human.device_id().as_str()];
        assert!(content.is_object());
        let raw = Raw::from_json_string(
            json!({"type":"m.room.encrypted","sender":"@worker:example.test","content":content})
                .to_string(),
        )
        .unwrap();
        let settings = DecryptionSettings {
            sender_device_trust_requirement: TrustRequirement::CrossSigned,
        };
        let changes = EncryptionSyncChanges {
            to_device_events: vec![raw],
            changed_devices: &Default::default(),
            one_time_keys_counts: &Default::default(),
            unused_fallback_keys: None,
            next_batch_token: None,
        };
        let (_, keys) = self
            .human
            .receive_sync_changes(changes, &settings)
            .await
            .unwrap();
        assert_eq!(keys.len(), 1);
    }
    pub async fn decrypt(&self, value: Value) -> Value {
        self.decrypt_in(value, room_id!("!project:example.test"))
            .await
    }
    pub async fn decrypt_in(&self, value: Value, room: &ruma::RoomId) -> Value {
        assert_ne!(value["session_id"], self.old_session);
        let raw=Raw::from_json_string(json!({"type":"m.room.encrypted","sender":"@worker:example.test","event_id":"$sent","origin_server_ts":1,"content":value}).to_string()).unwrap();
        let plain = self
            .human
            .decrypt_room_event(
                &raw,
                room,
                &DecryptionSettings {
                    sender_device_trust_requirement: TrustRequirement::CrossSigned,
                },
            )
            .await
            .unwrap();
        serde_json::from_str(plain.event.json().get()).unwrap()
    }
}

// Simulates an inconsistent old protected snapshot, not an external proof API.
// Production persistence validates before writing; this fixture bypasses it to
// ensure startup independently enforces the same complete-history invariants.
pub(super) async fn corrupt(sdk: &mut Sdk, variant: u8) {
    use crate::outgoing::state::Phase;
    let a = sdk.journal.outgoing.as_mut().unwrap();
    assert!(a.phase == Phase::Complete);
    assert!(a.writes.len() >= 2);
    match variant {
        10..=13 => super::file_publication::corrupt(a, variant),
        14 => {
            // Synthetic full retained catalog for admission bounds only. These
            // rows are not evidence of 64 actual network deliveries.
            let original = a.receipt().unwrap();
            sdk.journal.outgoing_receipts = (0..crate::outgoing::state::MAX_RECEIPTS)
                .map(|index| {
                    let mut receipt = original.clone();
                    receipt.id = format!("retained_{index}");
                    receipt
                })
                .collect();
            sdk.journal.outgoing = None;
        }
        0 => a.writes[0].response = None,
        1 => a.index = 0,
        2 => a.writes[0].room = true,
        3 => {
            a.writes.last_mut().unwrap().response =
                Some(json!({"event_id":"$accepted","extra":true}))
        }
        4 => a.route.thread_root = Some("$changed".into()),
        5 => a.phase = Phase::Ready,
        6 => a.keys_digest = Some("f".repeat(64)),
        7 => a.route.transport_generation = 0,
        8 => sdk.journal.outgoing_receipts.push(a.receipt().unwrap()),
        _ => panic!("unknown fixture"),
    }
    sdk.persist().await.unwrap();
}
