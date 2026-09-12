use crate::Error;
use hagency_core::{
    JSON_SAFE_MAX, canonical,
    custody::{Kind, Lane},
};
use hagency_store::outbound::LeasedDelivery;
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use std::fmt;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict, E> {
                Number::from_f64(v)
                    .map(|n| Strict(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(Strict(value)) = a.next_element()? {
                    values.push(value);
                }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut values = Map::new();
                while let Some(key) = a.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate key"));
                    }
                    let Strict(value) = a.next_value()?;
                    values.insert(key, value);
                }
                Ok(Strict(Value::Object(values)))
            }
        }
        d.deserialize_any(V)
    }
}
pub(crate) fn json(bytes: &[u8]) -> Result<Value, Error> {
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let Strict(value) = Strict::deserialize(&mut d).map_err(|_| Error::InvalidJson)?;
    d.end().map_err(|_| Error::InvalidJson)?;
    // serde limits parser recursion to 128; the retained transport contract is 64.
    canonical::encode_transport(&value).map_err(|_| Error::InvalidJson)?;
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Poll {
    v: u64,
    generation: u64,
    delivery: Option<Lease>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lease {
    id: String,
    lane: Lane,
    token: String,
    #[serde(rename = "expiresAt")]
    expires_at: String,
    kind: Kind,
    payload: Value,
}
pub(crate) fn poll(
    value: Value,
    generation: u64,
    lane: Lane,
) -> Result<Option<LeasedDelivery>, Error> {
    // Missing delivery is not an empty response. serde's Option alone permits it.
    if value.get("delivery").is_none() {
        return Err(Error::Wire);
    }
    let p: Poll = serde_json::from_value(value).map_err(|_| Error::Wire)?;
    if p.generation != generation {
        return Err(Error::Generation);
    }
    if p.v != 2 {
        return Err(Error::Wire);
    }
    let Some(d) = p.delivery else {
        return Ok(None);
    };
    if d.lane != lane
        || (d.lane == Lane::Matrix) != (d.kind == Kind::Transaction)
        || d.id.is_empty()
        || d.id.len() > 512
        || d.id.chars().any(char::is_control)
        || d.token.is_empty()
        || d.token.len() > 4096
        || d.token.chars().any(char::is_control)
        || !d.payload.is_object()
        || d.expires_at.len() > 40
    {
        return Err(Error::Wire);
    }
    let expires = OffsetDateTime::parse(&d.expires_at, &Rfc3339).map_err(|_| Error::Wire)?;
    let millis =
        u64::try_from(expires.unix_timestamp_nanos() / 1_000_000).map_err(|_| Error::Wire)?;
    if millis == 0 || millis > JSON_SAFE_MAX {
        return Err(Error::Wire);
    }
    if lane == Lane::Matrix
        && (d.payload.get("transactionId").and_then(Value::as_str) != Some(&d.id)
            || d.payload
                .get("body")
                .and_then(|v| v.get("events"))
                .and_then(Value::as_array)
                .is_none_or(|a| a.len() > 1000 || a.iter().any(|v| !v.is_object())))
    {
        return Err(Error::Wire);
    }
    Ok(Some(LeasedDelivery {
        machine_generation: generation,
        id: d.id,
        lane,
        kind: d.kind,
        payload: d.payload,
        token: d.token,
        expires_at_ms: millis,
    }))
}
pub(crate) fn accepted(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|m| m.len() == 1 && m.get("ok") == Some(&Value::Bool(true)))
}
pub(crate) fn stale_lease(value: &Value) -> bool {
    value.get("code").and_then(Value::as_str) == Some("stale_lease")
}
