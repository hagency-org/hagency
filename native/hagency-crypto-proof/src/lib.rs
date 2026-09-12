//! Offline feasibility proof, not a connected Matrix adapter or authorization layer.
//! Fresh Rust device state only. No NAPI store import or live server is attempted.
#[cfg(test)]
mod tests {
    use matrix_sdk_crypto::{DecryptionSettings, EncryptionSettings, OlmMachine, TrustRequirement};
    use matrix_sdk_sqlite::SqliteCryptoStore;
    use ruma::{device_id, events::room::message::RoomMessageEventContent, room_id, user_id};
    use serde_json::{Value, json};

    #[tokio::test]
    async fn crypto_device_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("crypto");
        hagency_store::private::directory(&path).unwrap();
        let key = [42u8; 32]; // Fixture key; never a deployed credential.
        let store = SqliteCryptoStore::open_with_key(&path, Some(&key))
            .await
            .unwrap();
        let user = user_id!("@fixture:example.test");
        let device = device_id!("RUST_FIXTURE");
        let room = room_id!("!fixture:example.test");
        let machine = OlmMachine::with_store(user, device, store, None)
            .await
            .unwrap();
        let identity = machine.identity_keys();
        let bootstrap = machine.bootstrap_cross_signing(false).await.unwrap();
        // Simulate the homeserver returning the exact public keys/signatures emitted by
        // this fixture device. The SDK verifies them; no trust flag is forced locally.
        let mut response = ruma::api::client::keys::get_keys::v3::Response::new();
        let signed = bootstrap.upload_signatures_req.signed_keys[user]
            .iter()
            .find(|(id, _)| *id == device.as_str())
            .unwrap()
            .1;
        let own = machine
            .get_device(user, device, None)
            .await
            .unwrap()
            .unwrap();
        let mut device_keys = serde_json::to_value(own.as_device_keys()).unwrap();
        drop(own); // Device handles also retain the old store; release before reopening.
        let signatures: Value = serde_json::from_str(signed.get()).unwrap();
        for (id, signature) in signatures["signatures"][user.as_str()].as_object().unwrap() {
            device_keys["signatures"][user.as_str()][id] = signature.clone();
        }
        response.device_keys.insert(
            user.to_owned(),
            [(
                device.to_owned(),
                serde_json::from_value(device_keys).unwrap(),
            )]
            .into(),
        );
        let keys = bootstrap.upload_signing_keys_req;
        response.master_keys.insert(
            user.to_owned(),
            serde_json::from_value(json!(keys.master_key.unwrap())).unwrap(),
        );
        response.self_signing_keys.insert(
            user.to_owned(),
            serde_json::from_value(json!(keys.self_signing_key.unwrap())).unwrap(),
        );
        response.user_signing_keys.insert(
            user.to_owned(),
            serde_json::from_value(json!(keys.user_signing_key.unwrap())).unwrap(),
        );
        let (request_id, _) = machine.query_keys_for_users([user]);
        machine
            .mark_request_as_sent(&request_id, &response)
            .await
            .unwrap();
        assert!(
            machine
                .get_device(user, device, None)
                .await
                .unwrap()
                .unwrap()
                .is_cross_signed_by_owner()
        );
        machine
            .share_room_key(room, std::iter::empty(), EncryptionSettings::default())
            .await
            .unwrap();
        let encrypted = machine
            .encrypt_room_event(
                room,
                RoomMessageEventContent::text_plain("小白：重启后仍能解密"),
            )
            .await
            .unwrap();
        let event = serde_json::from_value(json!({
            "event_id":"$fixture_event", "origin_server_ts":1234, "sender":user,
            "type":"m.room.encrypted", "content":encrypted.content,
        }))
        .unwrap();
        drop(machine);
        let store = SqliteCryptoStore::open_with_key(&path, Some(&key))
            .await
            .unwrap();
        assert!(
            OlmMachine::with_store(user, device_id!("WRONG_DEVICE"), store.clone(), None)
                .await
                .is_err()
        );
        let reopened = OlmMachine::with_store(user, device, store, None)
            .await
            .unwrap();
        assert_eq!(reopened.identity_keys(), identity);
        let settings = DecryptionSettings {
            sender_device_trust_requirement: TrustRequirement::CrossSigned,
        };
        let decrypted = reopened
            .decrypt_room_event(&event, room, &settings)
            .await
            .unwrap();
        let value: Value = serde_json::from_str(decrypted.event.json().get()).unwrap();
        assert_eq!(value["content"]["body"], "小白：重启后仍能解密");
        assert_eq!(value["sender"], user.as_str());
        drop(reopened);
        assert!(
            SqliteCryptoStore::open_with_key(&path, Some(&[43u8; 32]))
                .await
                .is_err()
        );
    }
}
