use hagency_core::canonical::{encode, encode_payload, payload_digest};
use serde_json::{Value, json};
#[test]
fn native_payload_number_vectors() {
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("../../fixtures/payloads.json")).unwrap();
    assert!(vectors.len() > 250);
    for (index, v) in vectors.iter().enumerate() {
        assert_eq!(
            encode_payload(&v["input"]).unwrap(),
            v["canonical"].as_str().unwrap(),
            "{index}"
        );
        assert_eq!(
            payload_digest(&v["input"]).unwrap(),
            v["sha256"].as_str().unwrap(),
            "{index}"
        );
    }
    assert_eq!(
        encode_payload(&serde_json::from_str("-0.0").unwrap()).unwrap(),
        "0"
    );
    for raw in ["1.25", "9007199254740992", "1e100"] {
        assert!(encode(&serde_json::from_str(raw).unwrap()).is_err());
    }
    assert!(encode_payload(&serde_json::from_str(r#"{"__proto__":{}}"#).unwrap()).is_err());
    let mut deep = Value::Null;
    for _ in 0..66 {
        deep = json!([deep]);
    }
    assert!(encode_payload(&deep).is_err());
}
