//! Offline crypto provisioning; not compiled into the host adapter.
use super::Sdk;
use matrix_sdk_crypto::{EncryptionSettings, types::requests::AnyOutgoingRequest};
use ruma::{api::client::keys::claim_keys, room_id, serde::Raw};
use serde_json::{Value, json};
pub(crate) struct Packet {
    pub query: Value,
    pub sync: Value,
}
pub(super) async fn prepare(sdk: &Sdk, contents: Vec<Value>, verified: bool) -> Packet {
    let (human, query) = super::crypto_fixture::verified_pair(sdk, verified).await;
    let guard = sdk.client.olm_machine().await;
    let receiver = guard.as_ref().unwrap();
    let outgoing = receiver.outgoing_requests().await.unwrap();
    let key = outgoing
        .iter()
        .find_map(|r| match r.request() {
            AnyOutgoingRequest::KeysUpload(r) => r.one_time_keys.iter().next(),
            _ => None,
        })
        .unwrap();
    let mut claim = claim_keys::v3::Response::new(Default::default());
    claim.one_time_keys.insert(
        receiver.user_id().to_owned(),
        [(
            receiver.device_id().to_owned(),
            [(key.0.clone(), key.1.clone())].into(),
        )]
        .into(),
    );
    let (id, _) = human
        .get_missing_sessions([receiver.user_id()].into_iter())
        .await
        .unwrap()
        .unwrap();
    human.mark_request_as_sent(&id, &claim).await.unwrap();
    let room = room_id!("!private:example.test");
    let shares = human
        .share_room_key(
            room,
            [receiver.user_id()].into_iter(),
            EncryptionSettings::default(),
        )
        .await
        .unwrap();
    let mut to_device = vec![];
    for share in shares {
        let messages = &share.messages[receiver.user_id()];
        assert_eq!(messages.len(), 1);
        let content: Value =
            serde_json::from_str(messages.values().next().unwrap().json().get()).unwrap();
        to_device.push(json!({"sender":human.user_id(),"type":share.event_type,"content":content}));
    }
    let mut events = vec![];
    for (i, content) in contents.into_iter().enumerate() {
        let raw = Raw::from_json_string(content.to_string()).unwrap();
        let cipher = human
            .encrypt_room_event_raw(room, "m.room.message", &raw)
            .await
            .unwrap();
        events.push(json!({"event_id":format!("$verdict{i}"),"origin_server_ts":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64,"sender":human.user_id(),"type":"m.room.encrypted","content":cipher.content}));
    }
    Packet {
        query: json!({"device_keys":query.device_keys,"master_keys":query.master_keys,"self_signing_keys":query.self_signing_keys,"user_signing_keys":query.user_signing_keys,"failures":{}}),
        sync: json!({"next_batch":"approval_first","rooms":{"join":{"!private:example.test":{"state":{"events":[]},"timeline":{"limited":false,"events":events}}}},"to_device":{"events":to_device}}),
    }
}
pub(super) async fn corrupt(sdk: &mut Sdk, variant: u8) {
    let mut value = serde_json::to_value(&sdk.journal).unwrap();
    let b = &mut value["approval"];
    match variant {
        0 => b["identity"] = json!("different crypto identity"),
        1 => b["token"] = json!("different-cursor"),
        2 => b["events"][0]["digest"] = json!("0".repeat(64)),
        3 => b["rooms"][0]["authority"]["owner_mxid"] = json!("@other:example.test"),
        4 => b["targets"][0]["request_digest"] = json!("b".repeat(64)),
        5 => {
            let outcome = json!({"Accepted":{"request_id":"approval_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","choice":"deny"}});
            value["approval"]["acknowledgements"] = json!([outcome.clone()]);
            value["approval_outcomes"] = json!([{"source":value["approval"]["events"][0]["source"],"digest":value["approval"]["events"][0]["digest"],"outcome":outcome}]);
        }
        6 => b["phase"] = json!("Prepared"),
        7 => b["events"] = json!([]),
        8 => b["events"][0]["proof"]["sender"] = json!("@other:example.test"),
        _ => panic!("fixture variant"),
    }
    // Write a structurally corrupt but correctly encrypted journal, bypassing only
    // the production validator in this test-only fault injector.
    let bytes = sdk.cipher.encrypt_value(&value).unwrap();
    sdk.client
        .state_store()
        .set_custom_value(super::JOURNAL, bytes)
        .await
        .unwrap();
}
