use crate::collector::fixtures as common;
use crate::{CancellationToken, Collector, Error};
mod fixture;
mod recovery;
use fixture::*;
use serde_json::json;

#[tokio::test]
async fn native_matrix_enrollment_fresh() {
    let mut f = Fixture::new().await;
    assert_eq!(f.run().await, Ok(()));
    assert_eq!(f.peer.writes.len(), 5);
    assert_eq!(f.peer.claims, 1);
    let writes = f.peer.writes.len();
    assert_eq!(f.run().await, Ok(()));
    assert_eq!(f.peer.writes.len(), writes);
    assert_eq!(f.peer.claims, 1);
    f.close().await;
}

#[tokio::test]
async fn native_matrix_enrollment_existing() {
    for variant in 0..4 {
        let mut f = Fixture::new().await;
        if variant == 0 {
            f.peer.query["master_keys"][crypto::SENDER] =
                f.peer.query["master_keys"][crypto::HUMAN].clone();
        } else if variant == 3 {
            f.peer.query["self_signing_keys"][crypto::SENDER] =
                f.peer.query["self_signing_keys"][crypto::HUMAN].clone();
        }
        let result = drive_with(
            &f.collector,
            &mut f.fake,
            &mut f.peer,
            &CancellationToken::new(),
            |request, peer, reply| {
                if variant == 1 && request.target == "/_matrix/client/v3/keys/upload" {
                    // A concurrent real server identity appears after the initial query.
                    // The fixture's ordinary-user signing endpoint itself refuses reset.
                    peer.query["master_keys"][crypto::SENDER] =
                        peer.query["master_keys"][crypto::HUMAN].clone();
                }
                if variant == 2 && request.target == "/_matrix/client/versions" {
                    reply.1 = json!({"versions":["v1.10"]});
                }
            },
        )
        .await;
        let expected = match variant {
            1 => Error::Unauthorized,
            2 => Error::Unsupported,
            _ => Error::Identity,
        };
        assert_eq!(result, Err(expected), "existing identity variant {variant}");
        assert_eq!(f.peer.writes.len(), usize::from(variant == 1));
        assert_eq!(f.peer.claims, 0);
        assert!(!f.base.available().await);
        assert_eq!(f.run().await, Err(expected));
        f.fake.no_request().await;
        f.close().await;
    }
    // A real SDK-generated local identity alone is also an existing account;
    // no protected enrollment receipt is forged to make it admissible.
    let mut f = Fixture::new().await;
    stop_sdk(&f.collector).await;
    let (machine, store) = recovery::open_crypto(&f.collector.inner.config).await;
    let original = machine.bootstrap_cross_signing(false).await.unwrap();
    drop(original);
    drop(machine);
    store.close().await.unwrap();
    drop(store);
    assert_eq!(f.run().await, Err(Error::Identity));
    assert!(f.peer.writes.is_empty());
    f.close().await;
}

#[tokio::test]
async fn native_matrix_enrollment_anchors() {
    let f = Fixture::new().await;
    let anchor = f.peer.anchor();
    assert!(
        f.base
            .config(&f.fake.endpoint)
            .with_fresh_account_enrollment(vec![(
                "@independent:remote.example".into(),
                anchor.clone()
            )])
            .is_ok()
    );
    for user in ["not-an-mxid", "@worker:example.test", "@bad:invalid server"] {
        assert!(
            f.base
                .config(&f.fake.endpoint)
                .with_fresh_account_enrollment(vec![(user.into(), anchor.clone())])
                .is_err()
        );
    }
    f.close().await;
    for variant in 0..6 {
        let mut f = Fixture::new().await;
        if variant == 0 {
            f.peer = crypto::Peer::new().await;
        }
        let result = drive_with(&f.collector, &mut f.fake, &mut f.peer,
            &CancellationToken::new(), |request, peer, reply| {
                if request.target == "/_matrix/client/v3/keys/query" {
                    let initial = peer.writes.is_empty();
                    match variant {
                        1 if initial => { reply.1["device_keys"][crypto::HUMAN][crypto::HUMAN_DEVICE]["signatures"] = json!({}); }
                        2 if initial => { reply.1["device_keys"][crypto::HUMAN][crypto::HUMAN_DEVICE]["algorithms"] = json!(["m.olm.v1.curve25519-aes-sha2"]); }
                        3 if initial => { reply.1["device_keys"].as_object_mut().unwrap().remove(crypto::HUMAN); }
                        5 if !initial => { reply.1["device_keys"]["@intruder:example.test"] = json!({}); }
                        _ => {}
                    }
                }
                if variant == 4 && request.target == "/_matrix/client/v3/keys/signatures/upload" {
                    reply.1 = json!({"failures":{crypto::HUMAN:{"errcode":"M_UNKNOWN"}}});
                }
            }).await;
        assert_eq!(result, Err(Error::Recipients), "anchor variant {variant}");
        assert_eq!(
            f.peer.writes.len(),
            match variant {
                4 => 3,
                5 => 4,
                _ => 0,
            }
        );
        assert_eq!(f.peer.claims, 0);
        assert!(!f.base.available().await);
        f.fake.no_request().await;
        f.close().await;
    }
}

#[tokio::test]
async fn native_matrix_enrollment_sessions() {
    let mut f = Fixture::new().await;
    assert_eq!(f.run().await, Ok(()));
    recovery::decrypt_from_original_sessions(&mut f).await;
    assert_eq!(f.peer.claims, 1);
    f.close().await;
    for variant in 0..4 {
        let mut f = Fixture::new().await;
        let result = drive_with(
            &f.collector,
            &mut f.fake,
            &mut f.peer,
            &CancellationToken::new(),
            |request, _, reply| {
                if request.target == "/_matrix/client/v3/keys/claim" {
                    match variant {
                        0 => {
                            reply.1["one_time_keys"][crypto::HUMAN] = json!({});
                        }
                        1 => {
                            reply.1["one_time_keys"]["@intruder:example.test"] = json!({});
                        }
                        2 => {
                            let key = reply.1["one_time_keys"][crypto::HUMAN][crypto::HUMAN_DEVICE]
                                .as_object_mut()
                                .unwrap()
                                .values_mut()
                                .next()
                                .unwrap();
                            key["signatures"] = json!({});
                        }
                        3 => {
                            reply.1["failures"] = json!({"example.test":{}});
                        }
                        _ => unreachable!(),
                    }
                }
            },
        )
        .await;
        assert_eq!(result, Err(Error::Recipients), "claim variant {variant}");
        assert_eq!(f.peer.claims, 1);
        assert_eq!(f.peer.writes.len(), 5);
        stop_sdk(&f.collector).await;
        assert!(matches!(
            sdk_status(&f.collector).await,
            Err(Error::OutcomeUnknown)
        ));
        f.fake.no_request().await;
        f.close().await;
    }
}

#[tokio::test]
async fn native_matrix_enrollment_current_scope() {
    for variant in 0..3 {
        let mut f = Fixture::new().await;
        let mut rejected = false;
        let result = drive_with(&f.collector, &mut f.fake, &mut f.peer,
            &CancellationToken::new(), |request, peer, reply| {
                if !peer.writes.is_empty() && !rejected {
                    if variant == 0 && request.target.ends_with("/whoami") {
                        reply.1["device_id"] = json!("OTHER_DEVICE"); rejected = true;
                    } else if variant == 1 && request.target.ends_with("/state") {
                        reply.1.as_array_mut().unwrap().push(json!({"type":"m.room.member","state_key":"@unexpected:example.test","content":{"membership":"join"}}));
                        rejected = true;
                    } else if variant == 2 && request.target.ends_with("/whoami") {
                        reply.0 = 401; reply.1 = json!({"errcode":"M_UNKNOWN_TOKEN"}); rejected = true;
                    }
                }
            }).await;
        assert!(rejected);
        assert_eq!(
            result,
            Err(match variant {
                0 => Error::Identity,
                1 => Error::Generation,
                _ => Error::Unauthorized,
            })
        );
        assert_eq!(f.peer.writes.len(), 1);
        assert_eq!(f.peer.claims, 0);
        assert!(!f.base.available().await);
        f.fake.no_request().await;
        f.close().await;
    }
}
