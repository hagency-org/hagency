//! Independent recipient and protocol fixture. This module never opens, seeds,
//! or obtains a reference to the service SDK. Its server query is populated only
//! by the recipient's own public material and actual service upload requests.
#![allow(dead_code)]
use matrix_sdk_crypto::{
    DecryptionSettings, EncryptionSyncChanges, OlmMachine, TrustRequirement, UserIdentity,
    types::requests::AnyOutgoingRequest,
};
use ruma::{RoomId, api::client::keys::get_keys, device_id, serde::Raw, user_id};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const SENDER: &str = "@worker:example.test";
pub const DEVICE: &str = "DEVICE_1";
pub const HUMAN: &str = "@owner:example.test";
pub const HUMAN_DEVICE: &str = "HUMAN";

pub struct Peer {
    sender: ruma::OwnedUserId,
    device: String,
    pub query: Value,
    pub writes: Vec<(String, Value)>,
    pub claims: usize,
    pub shares: usize,
    pub events: Vec<Value>,
    human: OlmMachine,
    one_time: BTreeMap<String, Value>,
    sender_master: Option<Value>,
    recipient_trusted: bool,
}

impl Peer {
    pub async fn new() -> Self {
        Self::for_sender(SENDER, DEVICE).await
    }
    pub async fn for_sender(sender: &str, sender_device: &str) -> Self {
        let human = OlmMachine::new(user_id!("@owner:example.test"), device_id!("HUMAN")).await;
        // Provision only the independent recipient. None of these original
        // requests or private identity handles can reach the service process.
        let bootstrap = human.bootstrap_cross_signing(false).await.unwrap();
        let upload = bootstrap.upload_keys_req.as_ref().unwrap();
        let AnyOutgoingRequest::KeysUpload(upload) = upload.request() else {
            panic!("recipient bootstrap did not generate its real device/OTKs")
        };
        assert!(!upload.one_time_keys.is_empty());
        let one_time = upload
            .one_time_keys
            .iter()
            .map(|(id, key)| (id.to_string(), serde_json::to_value(key).unwrap()))
            .collect();
        let device = serde_json::to_value(upload.device_keys.as_ref().unwrap()).unwrap();
        let signing = bootstrap.upload_signing_keys_req;
        let mut query = json!({
            "device_keys": {HUMAN: {HUMAN_DEVICE: device}},
            "master_keys": {HUMAN: signing.master_key.unwrap()},
            "self_signing_keys": {HUMAN: signing.self_signing_key.unwrap()},
            "user_signing_keys": {HUMAN: signing.user_signing_key.unwrap()},
            "failures": {}
        });
        merge_signatures(
            &mut query,
            &serde_json::to_value(bootstrap.upload_signatures_req.signed_keys).unwrap(),
        );
        let initial = query_response(&query);
        let (id, _) = human.query_keys_for_users([human.user_id()]);
        human.mark_request_as_sent(&id, &initial).await.unwrap();
        assert!(
            human
                .get_identity(human.user_id(), None)
                .await
                .unwrap()
                .unwrap()
                .is_verified()
        );
        Self {
            sender: sender.try_into().unwrap(),
            device: sender_device.into(),
            query,
            writes: Vec::new(),
            claims: 0,
            shares: 0,
            events: Vec::new(),
            human,
            one_time,
            sender_master: None,
            recipient_trusted: false,
        }
    }

    pub fn anchor(&self) -> String {
        let keys = self.query["master_keys"][HUMAN]["keys"]
            .as_object()
            .unwrap();
        assert_eq!(keys.len(), 1);
        keys.values().next().unwrap().as_str().unwrap().into()
    }

    /// Actual request-derived server state. The TLS test harness must check its
    /// original ordinary bearer credential before calling this protocol fixture.
    /// Unknown routes are returned to that harness, never silently acknowledged.
    pub async fn protocol(
        &mut self,
        method: &str,
        target: &str,
        body: &Value,
    ) -> Option<(u16, Value)> {
        if method == "GET" && target == "/_matrix/client/versions" {
            return Some((200, json!({"versions":["v1.11","v1.12"]})));
        }
        if method == "POST" && target == "/_matrix/client/v3/keys/query" {
            let requested = body["device_keys"].as_object().unwrap();
            assert!(!requested.is_empty() && requested.len() <= 2);
            let mut response = json!({
                "device_keys":{}, "master_keys":{}, "self_signing_keys":{},
                "user_signing_keys":{}, "failures":{}
            });
            for (user, selection) in requested {
                assert!([self.sender.as_str(), HUMAN].contains(&user.as_str()));
                assert!(
                    selection.as_array().unwrap().is_empty(),
                    "fixture expects all original devices"
                );
                response["device_keys"][user] = self.query["device_keys"]
                    .get(user)
                    .cloned()
                    .unwrap_or(json!({}));
                for field in ["master_keys", "self_signing_keys", "user_signing_keys"] {
                    // A real homeserver exposes user-signing keys only to their owner.
                    if field == "user_signing_keys" && user != self.sender.as_str() {
                        continue;
                    }
                    if let Some(value) = self.query[field].get(user) {
                        response[field][user] = value.clone();
                    }
                }
            }
            return Some((200, response));
        }
        if method != "POST" {
            return None;
        }
        match target {
            "/_matrix/client/v3/keys/upload" => {
                assert!(body.get("auth").is_none());
                assert_eq!(
                    self.writes.len(),
                    0,
                    "fresh device upload must be first and single"
                );
                let device = &body["device_keys"];
                assert_eq!(device["user_id"], self.sender.as_str());
                assert_eq!(device["device_id"], self.device);
                assert!(device["keys"].as_object().unwrap().len() >= 2);
                let otks = body["one_time_keys"].as_object().unwrap();
                assert!(!otks.is_empty());
                for (id, key) in otks {
                    assert!(id.starts_with("signed_curve25519:"));
                    assert!(
                        key["signatures"][self.sender.as_str()]
                            .as_object()
                            .is_some_and(|s| !s.is_empty())
                    );
                }
                self.query["device_keys"][self.sender.as_str()] =
                    json!({(self.device.as_str()): device});
                self.writes.push((target.into(), body.clone()));
                Some((
                    200,
                    json!({"one_time_key_counts":{"signed_curve25519":otks.len()}}),
                ))
            }
            "/_matrix/client/v3/keys/device_signing/upload" => {
                assert!(body.get("auth").is_none());
                assert_eq!(self.writes.len(), 1);
                let original = &body["master_key"];
                if let Some(existing) = self.query["master_keys"].get(self.sender.as_str())
                    && existing != original
                {
                    // Ordinary-client UIA race behavior; never accept a reset.
                    return Some((
                        401,
                        json!({"flows":[{"stages":["m.login.password"]}],"session":"fixture-uia"}),
                    ));
                }
                for (field, table) in [
                    ("master_key", "master_keys"),
                    ("self_signing_key", "self_signing_keys"),
                    ("user_signing_key", "user_signing_keys"),
                ] {
                    assert_eq!(body[field]["user_id"], self.sender.as_str());
                    assert_eq!(body[field]["keys"].as_object().unwrap().len(), 1);
                    self.query[table][self.sender.as_str()] = body[field].clone();
                }
                self.sender_master = Some(original.clone());
                self.writes.push((target.into(), body.clone()));
                Some((200, json!({})))
            }
            "/_matrix/client/v3/keys/signatures/upload" => {
                assert!(
                    (2..4).contains(&self.writes.len()),
                    "one own and one peer signature upload"
                );
                assert!(body.get("auth").is_none());
                merge_signatures(&mut self.query, body);
                self.writes.push((target.into(), body.clone()));
                Some((200, json!({"failures":{}})))
            }
            "/_matrix/client/v3/keys/claim" => {
                assert_eq!(self.writes.len(), 4);
                assert_eq!(self.claims, 0, "fresh session claim must be single");
                assert_eq!(
                    body["one_time_keys"],
                    json!({HUMAN:{HUMAN_DEVICE:"signed_curve25519"}})
                );
                let (id, key) = self.one_time.pop_first().unwrap();
                self.claims += 1;
                self.writes.push((target.into(), body.clone()));
                Some((
                    200,
                    json!({"one_time_keys":{HUMAN:{HUMAN_DEVICE:{id:key}}},"failures":{}}),
                ))
            }
            _ => None,
        }
    }

    /// Explicit recipient-side fixture operator provisioning. The original
    /// service master was captured from its authenticated actual signing upload.
    /// The recipient signs that exact public identity locally; this does not add
    /// a signature to server responses or modify any service trust/session state.
    async fn trust_original_sender(&mut self) {
        if self.recipient_trusted {
            return;
        }
        let master = self
            .sender_master
            .as_ref()
            .expect("no original service key upload");
        assert_eq!(
            master["keys"],
            self.query["master_keys"][self.sender.as_str()]["keys"]
        );
        let mut local = self.query.clone();
        let response = query_response(&local);
        let (id, _) = self
            .human
            .query_keys_for_users([self.human.user_id(), self.sender.as_ref()]);
        self.human
            .mark_request_as_sent(&id, &response)
            .await
            .unwrap();
        let UserIdentity::Other(sender) = self
            .human
            .get_identity(self.sender.as_ref(), None)
            .await
            .unwrap()
            .unwrap()
        else {
            panic!("independent recipient did not accept service identity")
        };
        let signature = sender.verify().await.unwrap();
        merge_signatures(
            &mut local,
            &serde_json::to_value(signature.signed_keys).unwrap(),
        );
        let response = query_response(&local);
        let (id, _) = self
            .human
            .query_keys_for_users([self.human.user_id(), self.sender.as_ref()]);
        self.human
            .mark_request_as_sent(&id, &response)
            .await
            .unwrap();
        assert!(
            self.human
                .get_identity(self.sender.as_ref(), None)
                .await
                .unwrap()
                .unwrap()
                .is_verified()
        );
        self.recipient_trusted = true;
    }

    pub async fn share(&mut self, value: Value) {
        self.trust_original_sender().await;
        assert_eq!(value["messages"].as_object().unwrap().len(), 1);
        assert_eq!(value["messages"][HUMAN].as_object().unwrap().len(), 1);
        let content = &value["messages"][HUMAN][HUMAN_DEVICE];
        assert!(content.is_object());
        let raw = Raw::from_json_string(
            json!({"type":"m.room.encrypted","sender":self.sender.as_str(),"content":content})
                .to_string(),
        )
        .unwrap();
        let (_, keys) = self
            .human
            .receive_sync_changes(
                EncryptionSyncChanges {
                    to_device_events: vec![raw],
                    changed_devices: &Default::default(),
                    one_time_keys_counts: &Default::default(),
                    unused_fallback_keys: None,
                    next_batch_token: None,
                },
                &DecryptionSettings {
                    sender_device_trust_requirement: TrustRequirement::CrossSigned,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            keys.len(),
            1,
            "actual signed Olm claim must deliver a decryptable Megolm key"
        );
        self.shares += 1;
    }

    pub async fn decrypt(&mut self, value: Value, room: &RoomId) -> Value {
        assert!(self.recipient_trusted && self.shares > 0);
        let raw = Raw::from_json_string(
            json!({
                "type":"m.room.encrypted", "sender":self.sender.as_str(), "event_id":"$file_received",
                "origin_server_ts":1, "content":value
            })
            .to_string(),
        )
        .unwrap();
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
        let event = serde_json::from_str(plain.event.json().get()).unwrap();
        self.events.push(event);
        self.events.last().unwrap().clone()
    }
}

/// Apply server signature upload semantics while retaining the original signed
/// object. The fixture refuses a signature request that substitutes key fields.
fn merge_signatures(query: &mut Value, request: &Value) {
    for (user, objects) in request.as_object().unwrap() {
        for (id, signed) in objects.as_object().unwrap() {
            let mut candidates = Vec::new();
            if query["device_keys"][user].get(id).is_some() {
                candidates.push("device_keys");
            }
            for table in ["master_keys", "self_signing_keys", "user_signing_keys"] {
                if query[table][user]["keys"]
                    .as_object()
                    .is_some_and(|keys| keys.values().any(|key| key.as_str() == Some(id.as_str())))
                {
                    candidates.push(table);
                }
            }
            assert_eq!(candidates.len(), 1, "unknown or colliding signed key ID");
            let current = if candidates[0] == "device_keys" {
                &mut query["device_keys"][user][id]
            } else {
                &mut query[candidates[0]][user]
            };
            for (field, value) in signed.as_object().unwrap() {
                if field != "signatures" && field != "unsigned" {
                    assert_eq!(
                        current.get(field),
                        Some(value),
                        "signature upload changed {field}"
                    );
                }
            }
            for (signer, signatures) in signed["signatures"].as_object().unwrap() {
                if current["signatures"].get(signer).is_none() {
                    current["signatures"][signer] = json!({});
                }
                for (id, value) in signatures.as_object().unwrap() {
                    current["signatures"][signer][id] = value.clone();
                }
            }
        }
    }
}

fn query_response(value: &Value) -> get_keys::v3::Response {
    let mut response = get_keys::v3::Response::new();
    response.device_keys = serde_json::from_value(value["device_keys"].clone()).unwrap();
    response.master_keys = serde_json::from_value(value["master_keys"].clone()).unwrap();
    response.self_signing_keys =
        serde_json::from_value(value["self_signing_keys"].clone()).unwrap();
    response.user_signing_keys =
        serde_json::from_value(value["user_signing_keys"].clone()).unwrap();
    response
}
