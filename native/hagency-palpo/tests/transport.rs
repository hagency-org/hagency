mod common;
use common::*;
use hagency_core::custody::Lane;
use hagency_palpo::{Adapter, CancellationToken, Error, HostConfig, Step};
use hagency_store::outbound::{Command, Reply};
use serde_json::json;
use std::{sync::Arc, time::Duration};

#[test]
fn native_outbound_http_authority_configuration() {
    let path = format!("/api/fleet/v2/{FLEET}");
    for origin in [
        "http://example.test",
        "http://localhost",
        "https://user:secret@example.test",
        "https://@example.test",
        "https://example.test:0",
        "ftp://example.test",
        "https://EXAMPLE.test",
        "https://example.test/..",
        "https://example.test\\",
        " https://example.test",
    ] {
        assert!(
            HostConfig::new(
                registration(),
                &format!("{origin}{path}"),
                TOKEN,
                31,
                limits()
            )
            .is_err(),
            "{origin}"
        );
    }
    for ending in ["/", "?token=secret", "#fragment"] {
        assert!(
            HostConfig::new(
                registration(),
                &format!("https://example.test{path}{ending}"),
                TOKEN,
                31,
                limits()
            )
            .is_err()
        );
    }
    for token in [
        "short",
        "machine token has spaces",
        "machine-token\r\ninject:secret",
    ] {
        assert!(
            HostConfig::new(
                registration(),
                &format!("https://example.test{path}"),
                token,
                31,
                limits()
            )
            .is_err()
        );
    }
    assert!(
        HostConfig::new(
            registration(),
            &format!("https://example.test{path}"),
            TOKEN,
            0,
            limits()
        )
        .is_err()
    );
    let mut bad = limits();
    bad.poll_wait = Duration::from_secs(26);
    assert!(
        HostConfig::new(
            registration(),
            &format!("https://example.test{path}"),
            TOKEN,
            31,
            bad
        )
        .is_err()
    );
}

#[tokio::test]
async fn native_outbound_http_authority_tls_and_redaction() {
    let mut fake = Fake::start(true).await;
    let (_dir, store) = store();
    let untrusted = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let error = untrusted.poll_once(Lane::Work, &cancel).await.unwrap_err();
    assert_eq!(error, Error::Transport);
    assert!(!format!("{error:?} {error}").contains(TOKEN));
    fake.no_request().await;
    let trusted = config(&fake.endpoint, 31)
        .with_root_pem(include_bytes!("fixtures/ca.pem"))
        .unwrap();
    let adapter = Adapter::attach(trusted, store.clone()).await.unwrap();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        let request = fake.next().await;
        assert_eq!(request.method, "GET");
        assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
        assert_eq!(request.headers["x-hagency-generation"], "31");
        assert_eq!(request.headers["accept-encoding"], "identity");
        assert!(!request.headers.contains_key("referer"));
        assert!(
            request
                .target
                .starts_with(&format!("/api/fleet/v2/{FLEET}/poll?lane=work&consumer="))
        );
        assert!(request.target.ends_with("&wait=0"));
        assert!(!request.target.contains(TOKEN));
        request.json(200, empty(31));
    });
    assert_eq!(result, Ok(Step::Empty));
    // Trusted CA does not disable hostname verification.
    let mismatch = fake.endpoint.replace("127.0.0.1", "localhost");
    let (_second_dir, second_store) = common::store();
    let mismatch = config(&mismatch, 31)
        .with_root_pem(include_bytes!("fixtures/ca.pem"))
        .unwrap();
    let mismatch = Adapter::attach(mismatch, second_store.clone())
        .await
        .unwrap();
    // A refused TLS/hostname handshake is classified by which side of the
    // bounded race in src/http.rs wins: `Transport` when the client observes
    // the refusal, `Timeout` when the peer's silent drop leaves the handshake
    // pending until the fixture's header budget. Both are documented refusals
    // with identical custody semantics (ADR-042, src/lib.rs), and the
    // production header budget is 5 s where this fixture allows 300 ms. What
    // must hold: the poll fails with a transport refusal, and the peer never
    // receives a request.
    match mismatch.poll_once(Lane::Work, &cancel).await {
        Err(Error::Transport | Error::Timeout) => {}
        other => panic!("mismatched hostname must be refused, got {other:?}"),
    }
    fake.no_request().await;
    second_store.shutdown().await.unwrap();
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_authority_redirect_and_status() {
    let mut fake = Fake::start(false).await;
    let mut destination = Fake::start(false).await;
    let (_dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.raw(
            format!(
                "HTTP/1.1 307 Redirect\r\nLocation: {}?token={TOKEN}\r\nContent-Length: 0\r\n\r\n",
                destination.endpoint
            )
            .into_bytes(),
        );
    });
    assert_eq!(result, Err(Error::Redirect));
    destination.no_request().await;
    for status in [204, 400, 401, 403, 409, 429, 500, 503] {
        let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
            fake.next()
                .await
                .json(status, json!({"error":TOKEN,"code":"opaque"}));
        });
        let error = if status == 401 || status == 403 {
            Error::Unauthorized
        } else {
            Error::Remote(status)
        };
        assert_eq!(result, Err(error));
        assert!(!format!("{result:?}").contains(TOKEN));
    }
    fake.no_request().await;
    store.shutdown().await.unwrap();
    fake.close().await;
    destination.close().await;
}

#[tokio::test]
async fn native_outbound_http_wire_invalid_envelopes_and_json() {
    let mut fake = Fake::start(false).await;
    let (_dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let good = delivery("matrix-1", Lane::Matrix, 31, "lease-original");
    let mut cases = vec![
        (b"{".to_vec(), Error::InvalidJson),
        (
            b"{\"v\":2,\"generation\":31,\"delivery\":null}{}".to_vec(),
            Error::InvalidJson,
        ),
        (
            b"{\"v\":2,\"v\":2,\"generation\":31,\"delivery\":null}".to_vec(),
            Error::InvalidJson,
        ),
        (
            b"{\"v\":2,\"generation\":31,\"delivery\":null,\"\\u0076\":2}".to_vec(),
            Error::InvalidJson,
        ),
        (
            serde_json::to_vec(&json!({"v":2,"generation":31})).unwrap(),
            Error::Wire,
        ),
        (serde_json::to_vec(&empty(30)).unwrap(), Error::Generation),
    ];
    for (key, value) in [("v", json!(1)), ("extra", json!(true))] {
        let mut v = good.clone();
        v[key] = value;
        cases.push((serde_json::to_vec(&v).unwrap(), Error::Wire));
    }
    for (key, value) in [
        ("lane", json!("work")),
        ("kind", json!("probe")),
        ("token", json!("")),
        ("expiresAt", json!("bad")),
        ("id", json!("different-transaction")),
        ("payload", json!([])),
    ] {
        let mut v = good.clone();
        v["delivery"][key] = value;
        cases.push((serde_json::to_vec(&v).unwrap(), Error::Wire));
    }
    let nested_duplicate = serde_json::to_string(&good)
        .unwrap()
        .replace("\"fraction\":2.75", "\"fraction\":2.75,\"fraction\":2.75");
    cases.push((nested_duplicate.into_bytes(), Error::InvalidJson));
    let mut deep = json!(0);
    for _ in 0..70 {
        deep = json!({"deep":deep});
    }
    let mut nested = good.clone();
    nested["delivery"]["payload"]["deep"] = deep;
    cases.push((serde_json::to_vec(&nested).unwrap(), Error::InvalidJson));
    for (bytes, error) in cases {
        let (result, _) = tokio::join!(adapter.poll_once(Lane::Matrix, &cancel), async {
            let request = fake.next().await;
            assert!(request.target.contains("/poll?"));
            request.raw(response(200, &bytes));
        });
        assert_eq!(result, Err(error));
    }
    let Reply::Head(head) = store
        .outbound(
            Command::Head {
                scope: adapter.scope(),
                lane: Lane::Matrix,
            },
            now(),
        )
        .await
        .unwrap()
    else {
        panic!("head");
    };
    assert!(head.is_none());
    fake.no_request().await;
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_wire_bytes_and_deadlines() {
    let mut fake = Fake::start(false).await;
    let (_dir, store) = store();
    let mut bounds = limits();
    bounds.poll_bytes = 512;
    let adapter = Adapter::attach(
        HostConfig::new(registration(), &fake.endpoint, TOKEN, 31, bounds).unwrap(),
        store.clone(),
    )
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let cases = [
        (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 99999\r\n\r\n".to_vec(), Error::BodyTooLarge),
        (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n".to_vec(), Error::Headers),
        (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Type: application/json\r\nContent-Length: 0\r\n\r\n".to_vec(), Error::Headers),
        (format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Oversized: {}\r\nContent-Length: 0\r\n\r\n", "x".repeat(17000)).into_bytes(), Error::Headers),
        (format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n", 600, "x".repeat(600)).into_bytes(), Error::BodyTooLarge),
    ];
    for (bytes, error) in cases {
        let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
            fake.next().await.raw(bytes);
        });
        assert_eq!(result, Err(error));
    }
    let valid = response(200, &serde_json::to_vec(&empty(31)).unwrap());
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next()
            .await
            .chunks(vec![(Duration::from_millis(500), valid)]);
    });
    assert_eq!(result, Err(Error::Timeout));
    let headers =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 39\r\n\r\n".to_vec();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.chunks(vec![
            (Duration::ZERO, headers),
            (Duration::from_millis(400), vec![b'{']),
        ]);
    });
    assert_eq!(result, Err(Error::Timeout));
    // Regular bytes cannot extend the absolute request deadline indefinitely.
    let mut trickle = vec![(
        Duration::ZERO,
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n"
            .to_vec(),
    )];
    for _ in 0..10 {
        trickle.push((Duration::from_millis(100), b"1\r\n \r\n".to_vec()));
    }
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.chunks(trickle);
    });
    assert_eq!(result, Err(Error::Timeout));
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_custody_persist_before_ack_and_restart() {
    let mut fake = Fake::start(false).await;
    let (dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let consumer = adapter.scope().consumer().to_owned();
    let cancel = CancellationToken::new();
    let original = delivery("tx-preserve", Lane::Matrix, 31, "lease-private-original");
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Matrix, &cancel), async {
        fake.next().await.json(200, original.clone());
        let request = fake.next().await;
        assert!(request.target.ends_with("/ack"));
        assert_eq!(
            request.value(),
            json!({"id":"tx-preserve","lane":"matrix","token":"lease-private-original"})
        );
        let saved = view(&store, &adapter, Lane::Matrix, "tx-preserve").await;
        assert_eq!(saved.lease_state, "unknown");
        assert_eq!(saved.receipt.generation, 7); // not machine generation 31
        assert_eq!(saved.origin_machine_generation, 31);
        // Simulate Palpo accepting the exact ACK then losing the response.
        drop(request);
    });
    assert_eq!(result, Err(Error::Transport));
    store.shutdown().await.unwrap();
    drop(adapter);
    drop(store);
    let store = open(&dir);
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    assert_eq!(adapter.scope().consumer(), consumer);
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Matrix, &cancel), async {
        let ack = fake.next().await;
        assert!(ack.target.ends_with("/ack")); // No new poll before resuming ACK.
        assert_eq!(ack.value()["token"], "lease-private-original");
        ack.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Ok(Step::Acknowledged));
    let work = start(&store, &adapter, Lane::Matrix, "tx-preserve").await;
    assert_eq!(work.payload, original["delivery"]["payload"]);
    assert_eq!(work.origin_machine_generation, 31);
    complete(&store, work).await;
    assert_eq!(
        view(&store, &adapter, Lane::Matrix, "tx-preserve")
            .await
            .processing_state,
        "done"
    );
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_custody_completed_redelivery_ack_and_conflict() {
    let mut fake = Fake::start(false).await;
    let (dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let original = delivery("req-done", Lane::Work, 31, "lease-1");
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.json(200, original.clone());
        fake.next().await.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Ok(Step::Acknowledged));
    complete(
        &store,
        start(&store, &adapter, Lane::Work, "req-done").await,
    )
    .await;
    let mut replay = original.clone();
    replay["delivery"]["token"] = json!("lease-2");
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.json(200, replay.clone());
        drop(fake.next().await);
    });
    assert_eq!(result, Err(Error::Transport));
    let Reply::Head(head) = store
        .outbound(
            Command::Head {
                scope: adapter.scope(),
                lane: Lane::Work,
            },
            now(),
        )
        .await
        .unwrap()
    else {
        panic!("head");
    };
    assert!(head.is_none()); // Regression: normal Head excludes done tombstones.
    store.shutdown().await.unwrap();
    drop(adapter);
    drop(store);
    let store = open(&dir);
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        let request = fake.next().await;
        assert!(request.target.ends_with("/ack"));
        assert_eq!(request.value()["token"], "lease-2");
        request.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Ok(Step::Acknowledged));
    let prior = view(&store, &adapter, Lane::Work, "req-done").await;
    assert_eq!(prior.processing_state, "done");
    replay["delivery"]["payload"]["units"] = json!(7.75);
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.json(200, replay);
    });
    assert_eq!(result, Err(Error::Conflict));
    assert_eq!(view(&store, &adapter, Lane::Work, "req-done").await, prior);
    fake.no_request().await;
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_custody_cancellation_stale_lease_and_rotation() {
    let mut fake = Fake::start(false).await;
    let (_dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let original = delivery("tx-cancel", Lane::Matrix, 31, "lease-before-cancel");
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Matrix, &cancel), async {
        fake.next().await.json(200, original.clone());
        let ack = fake.next().await;
        assert_eq!(
            view(&store, &adapter, Lane::Matrix, "tx-cancel")
                .await
                .lease_state,
            "unknown"
        );
        cancel.cancel();
        drop(ack);
    });
    assert_eq!(result, Err(Error::Cancelled));
    let cancel = CancellationToken::new();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Matrix, &cancel), async {
        let ack = fake.next().await;
        assert!(ack.target.ends_with("/ack"));
        ack.json(
            409,
            json!({"code":"stale_lease","error":"synthetic expired lease"}),
        );
    });
    assert_eq!(result, Ok(Step::Reclaim));
    let mut replacement = original.clone();
    replacement["delivery"]["token"] = json!("lease-replaced");
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Matrix, &cancel), async {
        fake.next().await.json(200, replacement);
        let ack = fake.next().await;
        assert_eq!(ack.value()["token"], "lease-replaced");
        drop(ack);
    });
    assert_eq!(result, Err(Error::Transport));
    let rotated = Adapter::attach(config(&fake.endpoint, 32), store.clone())
        .await
        .unwrap();
    let old = view(&store, &rotated, Lane::Matrix, "tx-cancel").await;
    assert_eq!(old.lease_state, "retired");
    assert_eq!(old.origin_machine_generation, 31);
    assert_eq!(
        adapter.poll_once(Lane::Matrix, &cancel).await,
        Err(Error::Generation)
    );
    assert_eq!(
        rotated.poll_once(Lane::Matrix, &cancel).await,
        Ok(Step::AwaitingConsumer)
    );
    fake.no_request().await; // Rotation never invents old remote ACK success.
    let work = start(&store, &rotated, Lane::Matrix, "tx-cancel").await;
    assert_eq!(work.payload, original["delivery"]["payload"]);
    assert_eq!(work.origin_machine_generation, 31);
    complete(&store, work).await;
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_custody_slow_ack_and_uncertain_attempt() {
    let mut fake = Fake::start(false).await;
    let (dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next()
            .await
            .json(200, delivery("slow-ack", Lane::Work, 31, "lease-slow"));
        fake.next().await.chunks(vec![(
            Duration::from_millis(600),
            response(200, b"{\"ok\":true}"),
        )]);
    });
    assert_eq!(result, Err(Error::Timeout));
    assert_eq!(
        view(&store, &adapter, Lane::Work, "slow-ack")
            .await
            .lease_state,
        "unknown"
    );
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Ok(Step::Acknowledged));
    let _started = start(&store, &adapter, Lane::Work, "slow-ack").await;
    store.shutdown().await.unwrap();
    drop(adapter);
    drop(store);
    let store = open(&dir);
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    assert_eq!(
        view(&store, &adapter, Lane::Work, "slow-ack")
            .await
            .processing_state,
        "unknown"
    );
    let Reply::Claim(ticket) = store
        .outbound(
            Command::Claim {
                scope: adapter.scope(),
                lane: Lane::Work,
                id: "slow-ack".into(),
                lease_ms: 30000,
            },
            now(),
        )
        .await
        .unwrap()
    else {
        panic!("claim");
    };
    // Same attempt ID returns its original capability for receipt inspection,
    // but neither that capability nor a new claim can repeat unknown effects.
    assert!(matches!(
        store.outbound(Command::Start(ticket.unwrap()), now()).await,
        Err(hagency_store::Error::State)
    ));
    let Reply::Claim(new_attempt) = store
        .outbound(
            Command::Claim {
                scope: adapter.scope(),
                lane: Lane::Work,
                id: "new-attempt".into(),
                lease_ms: 30000,
            },
            now(),
        )
        .await
        .unwrap()
    else {
        panic!("new claim");
    };
    assert!(new_attempt.is_none());
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_publication_frozen_restart_and_rotation() {
    let mut fake = Fake::start(false).await;
    let (dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let observations = json!({"heartbeat":true,"statuses":[{"v":1,"fleetId":FLEET,"requestId":"r1","state":"pending","targetProjectId":"p1","targetRoomId":"!target:example.test","sourceRoomId":"!reception:example.test","sourceEventId":"$request","role":"coding","requestedTokens":100000,"observedAt":"2020-01-01T00:00:00.000Z","ratio":0.125}]});
    adapter.freeze_update(observations.clone()).await.unwrap();
    let mut frozen = Vec::new();
    let (result, _) = tokio::join!(adapter.publish_once(&cancel), async {
        let request = fake.next().await;
        assert!(request.target.ends_with("/updates"));
        assert_eq!(request.method, "POST");
        let value = request.value();
        assert_eq!(value["v"], 2);
        assert_eq!(value["generation"], 31);
        assert_eq!(value["sequence"], 1);
        assert_eq!(value["statuses"], observations["statuses"]);
        frozen = request.body.clone();
        drop(request);
    });
    assert_eq!(result, Err(Error::Transport));
    assert_eq!(
        adapter.freeze_update(json!({"heartbeat":true})).await,
        Err(Error::Conflict)
    );
    let (result, snapshot) = store.shutdown_observed().await;
    if let Err(error) = result {
        panic!("publication-before-reopen shutdown failed: {error:?}; {snapshot:?}");
    }
    drop(adapter);
    drop(store);
    let store = open(&dir);
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let (result, _) = tokio::join!(adapter.publish_once(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.body, frozen);
        request.json(409, json!({"code":"sequence_conflict","error":TOKEN}));
    });
    assert_eq!(result, Err(Error::Remote(409)));
    let (result, _) = tokio::join!(adapter.publish_once(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.body, frozen);
        request.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Ok(Step::Published));
    assert_eq!(adapter.publish_once(&cancel).await, Ok(Step::NoPublication));
    adapter
        .freeze_update(json!({"heartbeat":true}))
        .await
        .unwrap();
    let mut replacement = None;
    let (result, _) = tokio::join!(adapter.publish_once(&cancel), async {
        let old = fake.next().await;
        assert_eq!(old.value()["sequence"], 2);
        let next_config = HostConfig::new(
            registration(),
            &fake.endpoint,
            "synthetic-ROTATED-machine-token",
            32,
            limits(),
        )
        .unwrap();
        let rotated = Adapter::attach(next_config, store.clone()).await.unwrap();
        rotated
            .freeze_update(json!({"heartbeat":true}))
            .await
            .unwrap();
        replacement = Some(rotated);
        old.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Err(Error::Generation));
    let rotated = replacement.unwrap();
    let (result, _) = tokio::join!(rotated.publish_once(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.headers["x-hagency-generation"], "32");
        assert_eq!(
            request.headers["authorization"],
            "Bearer synthetic-ROTATED-machine-token"
        );
        assert_eq!(request.value()["generation"], 32);
        assert_eq!(request.value()["sequence"], 1);
        request.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Ok(Step::Published));
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_lanes_blocked_matrix_independent_work_and_publish() {
    let mut fake = Fake::start(false).await;
    let (dir, store) = store();
    let adapter = Arc::new(
        Adapter::attach(config(&fake.endpoint, 31), store.clone())
            .await
            .unwrap(),
    );
    let cancel = CancellationToken::new();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Matrix, &cancel), async {
        fake.next().await.json(
            200,
            delivery("blocked-matrix", Lane::Matrix, 31, "lease-matrix"),
        );
        fake.next().await.json(200, json!({"ok":true}));
    });
    assert_eq!(result, Ok(Step::Acknowledged));
    let worker = {
        let adapter = adapter.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { adapter.run(&cancel).await })
    };
    let mut work = false;
    let mut published = false;
    for _ in 0..8 {
        let request = fake.next().await;
        if request.target.contains("/poll?") {
            assert!(
                request.target.contains("lane=work"),
                "Matrix must wait for host consumer"
            );
            if work {
                request.json(200, empty(31));
            } else {
                request.json(200, delivery("independent", Lane::Work, 31, "lease-work"));
            }
        } else if request.target.ends_with("/ack") {
            assert_eq!(request.value()["id"], "independent");
            work = true;
            request.json(200, json!({"ok":true}));
        } else {
            assert!(request.target.ends_with("/updates"));
            assert_eq!(request.value()["heartbeat"], true);
            assert!(request.value().get("probeReceipts").is_none());
            published = true;
            request.json(200, json!({"ok":true}));
        }
        if work && published {
            break;
        }
    }
    assert!(work && published);
    for attempt in 0..100 {
        if view(&store, &adapter, Lane::Work, "independent")
            .await
            .lease_state
            == "accepted"
        {
            break;
        }
        assert!(
            attempt < 99,
            "the host must receive and commit the successful ACK before cancellation"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    cancel.cancel();
    assert_eq!(worker.await.unwrap(), Ok(()));
    assert_eq!(
        view(&store, &adapter, Lane::Matrix, "blocked-matrix")
            .await
            .processing_state,
        "pending"
    );
    assert_eq!(
        view(&store, &adapter, Lane::Work, "independent")
            .await
            .lease_state,
        "accepted"
    );
    assert!(!dir.path().join("private/domain.sqlite3").exists());
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_lanes_busy_and_backoff() {
    let mut fake = Fake::start(false).await;
    let (_dir, store) = store();
    let adapter = Arc::new(
        Adapter::attach(config(&fake.endpoint, 31), store.clone())
            .await
            .unwrap(),
    );
    let cancel = CancellationToken::new();
    let first = {
        let adapter = adapter.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { adapter.poll_once(Lane::Matrix, &cancel).await })
    };
    let held = fake.next().await;
    assert_eq!(
        adapter.poll_once(Lane::Matrix, &cancel).await,
        Err(Error::Busy)
    );
    let (work, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.json(200, empty(31));
    });
    assert_eq!(work, Ok(Step::Empty));
    held.json(200, empty(31));
    assert_eq!(first.await.unwrap(), Ok(Step::Empty));
    let runner = {
        let adapter = adapter.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { adapter.run(&cancel).await })
    };
    let mut work_attempts = Vec::new();
    for _ in 0..24 {
        let request = fake.next().await;
        if request.target.contains("lane=work") {
            work_attempts.push(tokio::time::Instant::now());
            request.json(503, json!({"code":"queue_full"}));
            if work_attempts.len() == 3 {
                break;
            }
        } else if request.target.contains("/poll?") {
            request.json(200, empty(31));
        } else {
            request.json(200, json!({"ok":true}));
        }
    }
    assert_eq!(work_attempts.len(), 3);
    assert!(work_attempts[1].duration_since(work_attempts[0]) >= Duration::from_millis(35));
    assert!(work_attempts[2].duration_since(work_attempts[1]) >= Duration::from_millis(75));
    cancel.cancel();
    assert_eq!(runner.await.unwrap(), Ok(()));
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_outbound_http_authority_ignores_environment_proxies() {
    const CHILD: &str = "HAGENCY_SYNTHETIC_PROXY_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_outbound_http_authority_ignores_environment_proxies",
            ])
            .env(CHILD, "1")
            .env("HTTP_PROXY", "http://127.0.0.1:1")
            .env("HTTPS_PROXY", "http://127.0.0.1:1")
            .env("ALL_PROXY", "http://127.0.0.1:1")
            .env("http_proxy", "http://127.0.0.1:1")
            .env("https_proxy", "http://127.0.0.1:1")
            .env("all_proxy", "http://127.0.0.1:1")
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let mut fake = Fake::start(false).await;
    let (_dir, store) = store();
    let adapter = Adapter::attach(config(&fake.endpoint, 31), store.clone())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let (result, _) = tokio::join!(adapter.poll_once(Lane::Work, &cancel), async {
        fake.next().await.json(200, empty(31));
    });
    assert_eq!(result, Ok(Step::Empty));
    store.shutdown().await.unwrap();
    fake.close().await;
}
