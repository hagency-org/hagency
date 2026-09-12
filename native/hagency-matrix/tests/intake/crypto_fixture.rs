//! Test-only offline key exchange. No HTTP registration/publication or trust
//! constructor exists in production. All event ciphertext comes from the SDK.
use super::Sdk;
use matrix_sdk_crypto::{
    EncryptionSettings, OlmMachine, UserIdentity, types::requests::AnyOutgoingRequest,
};
use ruma::{
    api::client::keys::{claim_keys, get_keys},
    device_id,
    events::room::message::RoomMessageEventContent,
    room_id, user_id,
};
use serde_json::{Value, json};

async fn public_keys(machine: &OlmMachine) -> get_keys::v3::Response {
    let user = machine.user_id();
    let device = machine.device_id();
    let bootstrap = machine.bootstrap_cross_signing(false).await.unwrap();
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
    let mut keys = serde_json::to_value(own.as_device_keys()).unwrap();
    let signatures: Value = serde_json::from_str(signed.get()).unwrap();
    for (id, value) in signatures["signatures"][user.as_str()].as_object().unwrap() {
        keys["signatures"][user.as_str()][id] = value.clone();
    }
    let mut response = get_keys::v3::Response::new();
    response.device_keys.insert(
        user.to_owned(),
        [(device.to_owned(), serde_json::from_value(keys).unwrap())].into(),
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
    response
}

pub(super) async fn verified_pair(
    sdk: &Sdk,
    verified: bool,
) -> (OlmMachine, get_keys::v3::Response) {
    let guard = sdk.client.olm_machine().await;
    let receiver = guard.as_ref().unwrap();
    let human = OlmMachine::new(user_id!("@owner:example.test"), device_id!("HUMAN")).await;
    let mut query = public_keys(receiver).await;
    let h = public_keys(&human).await;
    query.device_keys.extend(h.device_keys);
    query.master_keys.extend(h.master_keys);
    query.self_signing_keys.extend(h.self_signing_keys);
    query.user_signing_keys.extend(h.user_signing_keys);
    for machine in [receiver, &human] {
        let (id, _) = machine.query_keys_for_users([receiver.user_id(), human.user_id()]);
        machine.mark_request_as_sent(&id, &query).await.unwrap();
    }
    if verified {
        let own = receiver
            .get_identity(receiver.user_id(), None)
            .await
            .unwrap()
            .unwrap();
        let UserIdentity::Own(own) = own else {
            panic!("expected owned identity")
        };
        own.verify().await.unwrap();
        let identity = receiver
            .get_identity(human.user_id(), None)
            .await
            .unwrap()
            .unwrap();
        let UserIdentity::Other(identity) = identity else {
            panic!("human identity must be distinct")
        };
        let upload = identity.verify().await.unwrap();
        let signed = upload.signed_keys[human.user_id()].iter().next().unwrap().1;
        query.master_keys.insert(
            human.user_id().to_owned(),
            serde_json::from_str(signed.get()).unwrap(),
        );
        let (id, _) = receiver.query_keys_for_users([human.user_id()]);
        receiver.mark_request_as_sent(&id, &query).await.unwrap();
        assert!(
            receiver
                .get_identity(human.user_id(), None)
                .await
                .unwrap()
                .unwrap()
                .is_verified()
        );
    }
    (human, query)
}

pub(super) async fn encrypted_human(sdk: &Sdk, verified: bool, count: usize) -> Value {
    assert!((1..=2).contains(&count));
    let values = (0..count).map(|i| json!({"event_id":if i==0 {"$encrypted"} else {"$encrypted_new"},"content":RoomMessageEventContent::text_plain("小白：已验证的私聊，无需提及")})).collect();
    encrypted_contents(sdk, verified, values, true).await
}

pub(super) async fn encrypted_contents(
    sdk: &Sdk,
    verified: bool,
    contents: Vec<Value>,
    rotate: bool,
) -> Value {
    assert!(!contents.is_empty() && contents.len() <= 100);
    let (human, _) = verified_pair(sdk, verified).await;
    let guard = sdk.client.olm_machine().await;
    let receiver = guard.as_ref().unwrap();
    let outgoing = receiver.outgoing_requests().await.unwrap();
    let key = outgoing
        .iter()
        .find_map(|r| match r.request() {
            AnyOutgoingRequest::KeysUpload(request) => request.one_time_keys.iter().next(),
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
    let room = room_id!("!project:example.test");
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
        to_device
            .push(json!({"sender":human.user_id(), "type":share.event_type, "content":content}));
    }
    assert!(!to_device.is_empty());
    let mut events = vec![];
    for (i, value) in contents.iter().enumerate() {
        if rotate && i > 0 {
            // A newly sent message uses a fresh ordinary outbound Megolm session.
            // The receiver's earlier session/proof and journal are never reset.
            assert!(human.discard_room_key(room).await.unwrap());
            for share in human
                .share_room_key(
                    room,
                    [receiver.user_id()].into_iter(),
                    EncryptionSettings::default(),
                )
                .await
                .unwrap()
            {
                let messages = &share.messages[receiver.user_id()];
                assert_eq!(messages.len(), 1);
                let content: Value =
                    serde_json::from_str(messages.values().next().unwrap().json().get()).unwrap();
                to_device.push(
                    json!({"sender":human.user_id(),"type":share.event_type,"content":content}),
                );
            }
        }
        let encrypted = human
            .encrypt_room_event_raw(
                room,
                "m.room.message",
                &ruma::serde::Raw::from_json_string(
                    serde_json::to_string(&value["content"]).unwrap(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        events.push(json!({"event_id":value["event_id"],"origin_server_ts":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64,"sender":human.user_id(),"type":"m.room.encrypted","content":encrypted.content}));
    }
    json!({"next_batch":"encrypted", "rooms":{"join":{"!project:example.test":{"state":{"events":[]},"timeline":{"limited":false,"events":events}}}},"to_device":{"events":to_device}})
}

pub(super) async fn trust_human(sdk: &Sdk) {
    let guard = sdk.client.olm_machine().await;
    let receiver = guard.as_ref().unwrap();
    let UserIdentity::Own(own) = receiver
        .get_identity(receiver.user_id(), None)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("owned identity")
    };
    own.verify().await.unwrap();
    let user = user_id!("@owner:example.test");
    let UserIdentity::Other(other) = receiver.get_identity(user, None).await.unwrap().unwrap()
    else {
        panic!("human identity")
    };
    let upload = other.verify().await.unwrap();
    let signed = upload.signed_keys[user].iter().next().unwrap().1;
    let mut response = get_keys::v3::Response::new();
    response
        .master_keys
        .insert(user.to_owned(), serde_json::from_str(signed.get()).unwrap());
    response.self_signing_keys.insert(
        user.to_owned(),
        serde_json::from_value(serde_json::to_value(other.self_signing_key().as_ref()).unwrap())
            .unwrap(),
    );
    let device = receiver
        .get_device(user, device_id!("HUMAN"), None)
        .await
        .unwrap()
        .unwrap();
    response.device_keys.insert(
        user.to_owned(),
        [(
            device_id!("HUMAN").to_owned(),
            serde_json::from_value(serde_json::to_value(device.as_device_keys()).unwrap()).unwrap(),
        )]
        .into(),
    );
    let (id, _) = receiver.query_keys_for_users([user]);
    receiver.mark_request_as_sent(&id, &response).await.unwrap();
    assert!(
        receiver
            .get_identity(user, None)
            .await
            .unwrap()
            .unwrap()
            .is_verified()
    );
}
