//! Fresh authenticated key-query validation shared by outgoing and approval intake.
use crate::Error;
use matrix_sdk_crypto::{OlmMachine, UserIdentity};
use ruma::{
    OwnedUserId,
    api::{IncomingResponse, client::keys::get_keys},
};
use serde_json::Value;
use std::collections::BTreeSet;
pub(super) async fn accept(
    machine: &OlmMachine,
    users: &[OwnedUserId],
    query_id: &str,
    response: &Value,
) -> Result<BTreeSet<(String, String)>, Error> {
    validate_keys_shape(users, response)?;
    let query = get_keys::v3::Response::try_from_http_response(http::Response::new(
        response.to_string().into_bytes(),
    ))
    .map_err(|_| Error::Wire)?;
    let own = machine.identity_keys();
    let own_keys =
        &response["device_keys"][machine.user_id().as_str()][machine.device_id().as_str()]["keys"];
    if own_keys
        .get(format!("curve25519:{}", machine.device_id()))
        .and_then(Value::as_str)
        != Some(own.curve25519.to_base64().as_str())
        || own_keys
            .get(format!("ed25519:{}", machine.device_id()))
            .and_then(Value::as_str)
            != Some(own.ed25519.to_base64().as_str())
    {
        return Err(Error::Identity);
    }
    machine
        .mark_request_as_sent(query_id.into(), &query)
        .await
        .map_err(|_| Error::OutcomeUnknown)?;
    let mut recipients = BTreeSet::new();
    for user in users {
        let identity = machine
            .get_identity(user, None)
            .await
            .map_err(|_| Error::OutcomeUnknown)?
            .ok_or(Error::Recipients)?;
        if !identity.is_verified() {
            return Err(Error::Recipients);
        }
        // A malformed fresh entry may be ignored by the SDK while a cached
        // verified identity survives. Require the supplied authority fields
        // to be exactly the identity the SDK accepted, not just present.
        let (master, signing) = match &identity {
            UserIdentity::Own(identity) => (
                serde_json::to_value(identity.master_key().as_ref()),
                serde_json::to_value(identity.self_signing_key().as_ref()),
            ),
            UserIdentity::Other(identity) => (
                serde_json::to_value(identity.master_key().as_ref()),
                serde_json::to_value(identity.self_signing_key().as_ref()),
            ),
        };
        let master = master.map_err(|_| Error::Storage)?;
        let signing = signing.map_err(|_| Error::Storage)?;
        for (fresh, accepted) in [
            (&response["master_keys"][user.as_str()], &master),
            (&response["self_signing_keys"][user.as_str()], &signing),
        ] {
            if !same_fields(fresh, accepted, &["user_id", "usage", "keys", "signatures"]) {
                return Err(Error::Recipients);
            }
        }
        let devices = machine
            .get_user_devices(user, None)
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        let fresh = response["device_keys"][user.as_str()]
            .as_object()
            .ok_or(Error::Wire)?;
        let mut seen = BTreeSet::new();
        for device in devices.devices() {
            let id = device.device_id().as_str();
            if !device.is_verified() || !device.is_cross_signed_by_owner() {
                return Err(Error::Recipients);
            }
            let raw = fresh.get(id).ok_or(Error::Recipients)?;
            let accepted =
                serde_json::to_value(device.as_device_keys()).map_err(|_| Error::Storage)?;
            if !same_fields(
                raw,
                &accepted,
                &["user_id", "device_id", "algorithms", "keys", "signatures"],
            ) || raw["user_id"].as_str() != Some(user.as_str())
                || raw["device_id"].as_str() != Some(id)
            {
                return Err(Error::Recipients);
            }
            seen.insert(id.to_string());
            if user == machine.user_id() && device.device_id() == machine.device_id() {
                let own = machine.identity_keys();
                if device.curve25519_key() != Some(own.curve25519)
                    || device.ed25519_key() != Some(own.ed25519)
                {
                    return Err(Error::Identity);
                }
            } else {
                recipients.insert((user.to_string(), id.to_string()));
            }
        }
        if seen.len() != fresh.len() || seen.is_empty() {
            return Err(Error::Recipients);
        }
    }
    if response["device_keys"][machine.user_id().as_str()]
        .get(machine.device_id().as_str())
        .is_none()
    {
        return Err(Error::Identity);
    }
    Ok(recipients)
}
fn validate_keys_shape(users: &[OwnedUserId], value: &Value) -> Result<(), Error> {
    if value
        .get("failures")
        .is_some_and(|v| v.as_object().is_none_or(|o| !o.is_empty()))
    {
        return Err(Error::Recipients);
    }
    let keys = value
        .get("device_keys")
        .and_then(Value::as_object)
        .ok_or(Error::Wire)?;
    if keys.keys().cloned().collect::<BTreeSet<_>>()
        != users
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>()
    {
        return Err(Error::Recipients);
    }
    let mut count = 0;
    for (user, devices) in keys {
        count += devices.as_object().ok_or(Error::Wire)?.len();
        if value.get("master_keys").and_then(|v| v.get(user)).is_none()
            || value
                .get("self_signing_keys")
                .and_then(|v| v.get(user))
                .is_none()
        {
            return Err(Error::Recipients);
        }
    }
    if count > 64 {
        return Err(Error::Capacity);
    }
    Ok(())
}

pub(super) fn same_fields(fresh: &Value, accepted: &Value, fields: &[&str]) -> bool {
    fresh.is_object()
        && accepted.is_object()
        && fields
            .iter()
            .all(|key| fresh.get(*key).is_some() && fresh.get(*key) == accepted.get(*key))
}

/// Only the explicit fresh-account enrollment calls this pre-verification
/// validator. It does not replace `accept` at any encryption boundary.
pub(super) async fn anchored_initial(
    machine: &OlmMachine,
    users: &[OwnedUserId],
    query_id: &str,
    response: &Value,
    anchors: &std::collections::BTreeMap<String, String>,
) -> Result<(), Error> {
    let own = machine.user_id().as_str();
    if response["device_keys"][own]
        .as_object()
        .is_none_or(|v| !v.is_empty())
        || ["master_keys", "self_signing_keys", "user_signing_keys"]
            .iter()
            .any(|field| response.get(*field).and_then(|m| m.get(own)).is_some())
    {
        return Err(Error::Identity);
    }
    let peers = users
        .iter()
        .filter(|u| u.as_str() != own)
        .cloned()
        .collect::<Vec<_>>();
    let mut peer_response = response.clone();
    peer_response["device_keys"]
        .as_object_mut()
        .ok_or(Error::Wire)?
        .remove(own);
    validate_keys_shape(&peers, &peer_response)?;
    for field in ["master_keys", "self_signing_keys", "user_signing_keys"] {
        if response.get(field).is_some_and(|m| {
            m.as_object().is_none_or(|m| {
                m.keys()
                    .any(|u| !users.iter().any(|expected| expected.as_str() == u))
            })
        }) {
            return Err(Error::Recipients);
        }
    }
    let query = get_keys::v3::Response::try_from_http_response(http::Response::new(
        response.to_string().into_bytes(),
    ))
    .map_err(|_| Error::Wire)?;
    machine
        .mark_request_as_sent(query_id.into(), &query)
        .await
        .map_err(|_| Error::OutcomeUnknown)?;
    for user in &peers {
        let Some(UserIdentity::Other(identity)) = machine
            .get_identity(user, None)
            .await
            .map_err(|_| Error::OutcomeUnknown)?
        else {
            return Err(Error::Recipients);
        };
        let master =
            serde_json::to_value(identity.master_key().as_ref()).map_err(|_| Error::Storage)?;
        let signing = serde_json::to_value(identity.self_signing_key().as_ref())
            .map_err(|_| Error::Storage)?;
        let anchor = anchors.get(user.as_str()).ok_or(Error::Recipients)?;
        if master["keys"] != serde_json::json!({format!("ed25519:{anchor}"):anchor}) {
            return Err(Error::Recipients);
        }
        for (field, accepted) in [("master_keys", &master), ("self_signing_keys", &signing)] {
            if !same_fields(
                &response[field][user.as_str()],
                accepted,
                &["user_id", "usage", "keys", "signatures"],
            ) {
                return Err(Error::Recipients);
            }
        }
        let devices = machine
            .get_user_devices(user, None)
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        let fresh = response["device_keys"][user.as_str()]
            .as_object()
            .ok_or(Error::Recipients)?;
        let mut seen = BTreeSet::new();
        for device in devices.devices() {
            let id = device.device_id().as_str();
            let raw = fresh.get(id).ok_or(Error::Recipients)?;
            let accepted =
                serde_json::to_value(device.as_device_keys()).map_err(|_| Error::Storage)?;
            let algorithms = raw["algorithms"].as_array().ok_or(Error::Recipients)?;
            if !device.is_cross_signed_by_owner()
                || !same_fields(
                    raw,
                    &accepted,
                    &["user_id", "device_id", "algorithms", "keys", "signatures"],
                )
                || raw["user_id"].as_str() != Some(user.as_str())
                || raw["device_id"].as_str() != Some(id)
                || algorithms.len() != 2
                || !algorithms.contains(&serde_json::json!("m.olm.v1.curve25519-aes-sha2"))
                || !algorithms.contains(&serde_json::json!("m.megolm.v1.aes-sha2"))
                || [&master, &signing].iter().any(|key| {
                    key["keys"]
                        .as_object()
                        .is_none_or(|keys| keys.values().any(|v| v.as_str() == Some(id)))
                })
            {
                return Err(Error::Recipients);
            }
            seen.insert(id.to_owned());
        }
        if seen.is_empty() || seen.len() != fresh.len() {
            return Err(Error::Recipients);
        }
    }
    Ok(())
}
