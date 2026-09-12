#[path = "palpo_service/fixture.rs"]
mod fixture;
#[allow(dead_code)] // Reuse the existing bounded real HTTPS peer and trust root.
#[path = "../../hagency-palpo/tests/common/mod.rs"]
mod peer;
use fixture::*;
use hagency::bootstrap::{Bootstrap, Options};
use hagency_palpo::CancellationToken;
use serde_json::json;
use std::{fs, time::Duration};

#[tokio::test]
async fn native_palpo_service_executable_catalog() {
    let mut f = Fixture::new(true, false).await;
    let mut child = f.launch(true);
    let first = f.publication().await;
    assert!(offers(&check(&first, 1)).is_empty());
    let status = f.capabilities().await;
    assert_eq!(status["palpo_publication"]["state"], "running");
    assert_eq!(status["development_execution"]["state"], "disabled");
    for field in [
        "agent_execution",
        "palpo_transport",
        "matrix_crypto",
        "project_request_transport",
        "production_api_parity",
    ] {
        assert_eq!(status[field], false);
    }
    assert!(!status.to_string().contains(peer::TOKEN));
    assert!(!status.to_string().contains(f.state.to_str().unwrap()));
    f.request("POST", "resources", Some(resource_body(true)))
        .await;
    first.json(200, json!({"ok":true}));
    let second = f.publication().await;
    let value = check(&second, 2);
    assert!(!offers(&value).is_empty());
    for offer in offers(&value) {
        assert_eq!(offer["resources"][0]["id"], resource().id());
        let role = offer["role"].as_str().unwrap();
        f.request(
            "POST",
            &format!("roles/{role}/publication"),
            Some(json!({"published":false})),
        )
        .await;
    }
    // A changed file is not reloaded into this original owner's identity/token.
    f.rewrite(|v| v["machine_generation"] = json!(99));
    fs::write(
        f.state.join("palpo.machine_token"),
        b"different-configured-token",
    )
    .unwrap();
    second.json(200, json!({"ok":true}));
    let third = f.publication().await;
    assert!(offers(&check(&third, 3)).is_empty());
    third.json(200, json!({"ok":true}));
    f.no_runner();
    #[cfg(unix)]
    {
        child.graceful().await;
        f.reopen();
    }
    #[cfg(not(unix))]
    let _ = &mut child; // actual Windows signal qualification is separate
    drop(child);
    f.fake.close().await;
}

#[tokio::test]
async fn native_palpo_service_original_publication() {
    let mut f = Fixture::new(true, true).await;
    let mut child = f.launch(true);
    let first = f.publication().await;
    assert!(!offers(&check(&first, 1)).is_empty());
    let original = first.body.clone();
    let pending = f.pending();
    assert_eq!(pending.0, 1);
    assert_eq!(pending.2.as_bytes(), original);
    assert_eq!(pending.3, "unknown");
    f.request("POST", "resources", Some(resource_body(false)))
        .await;
    drop(first); // original bytes reached real HTTPS; no acknowledgment
    let retry = f.publication().await;
    assert_eq!(retry.body, original);
    assert_eq!(f.pending(), pending);
    check(&retry, 1);
    retry.json(200, json!({"ok":true}));
    let next = f.publication().await;
    assert!(offers(&check(&next, 2)).is_empty());
    next.json(200, json!({"ok":true}));
    #[cfg(unix)]
    child.graceful().await;
    #[cfg(not(unix))]
    let _ = &mut child;
    drop(child);
    f.fake.close().await;
}

#[tokio::test]
async fn native_palpo_service_configuration_refusal() {
    for case in [
        "disabled",
        "malformed",
        "unknown_field",
        "duplicate_field",
        "http",
        "short_token",
        "oversized",
        "directory",
        "mismatch",
        "missing",
    ] {
        let mut f = Fixture::new(case != "missing", false).await;
        let path = f.state.join("palpo-transport.json");
        match case {
            "disabled" | "malformed" => fs::write(&path, b"{").unwrap(),
            "unknown_field" => f.rewrite(|v| v["registration_fingerprint"] = json!("a".repeat(64))),
            "duplicate_field" => {
                let value = fs::read_to_string(&path).unwrap();
                fs::write(&path, value.replacen('{', "{\"profile\":\"duplicate\",", 1)).unwrap();
            }
            "http" => f.rewrite(|v| {
                v["endpoint"] = json!(
                    v["endpoint"]
                        .as_str()
                        .unwrap()
                        .replacen("https:", "http:", 1)
                )
            }),
            "short_token" => fs::write(f.state.join("palpo.machine_token"), b"short").unwrap(),
            "oversized" => fs::write(&path, vec![b' '; 16 * 1024 + 1]).unwrap(),
            "directory" => {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
            "mismatch" => f.rewrite(|v| v["registration"]["generation"] = json!(8)),
            "missing" => {}
            _ => unreachable!(),
        }
        let mut child = f.launch(case != "disabled");
        if case == "disabled" {
            let status = f.capabilities().await;
            assert_eq!(status["palpo_publication"]["state"], "disabled");
            assert_eq!(status["palpo_publication"]["configured"], false);
        } else if matches!(case, "mismatch" | "missing") {
            let status = f.terminal().await;
            assert_eq!(status["palpo_publication"]["state"], "unavailable");
            assert_eq!(
                status["palpo_publication"]["error"],
                if case == "mismatch" {
                    "generation"
                } else {
                    "custody"
                }
            );
        } else {
            child.refused().await;
        }
        f.fake.no_request().await;
        f.untouched_registration(case != "missing");
        f.no_runner();
        #[cfg(unix)]
        if matches!(case, "disabled" | "mismatch" | "missing") {
            child.graceful().await;
        }
        drop(child);
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_palpo_service_cancel_custody() {
    let mut f = Fixture::new(true, true).await;
    let mut bootstrap = Bootstrap::open_with_options(
        &f.state,
        f.address,
        16,
        Options {
            development_driver: false,
            palpo_transport: true,
        },
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let serving = tokio::spawn(Box::pin(async move {
        let result = bootstrap.serve(&signal).await;
        (result, bootstrap)
    }));
    let held = f.publication().await;
    check(&held, 1);
    // Hold one original request from each inbound lane too. Neither can issue
    // another poll while its own request is unacknowledged by this HTTPS peer.
    let matrix_or_work = f.fake.next().await;
    let other_lane = f.fake.next().await;
    assert!(matrix_or_work.target.contains("/poll?"));
    assert!(other_lane.target.contains("/poll?"));
    assert_ne!(matrix_or_work.target, other_lane.target);
    let original = f.pending();
    assert_eq!(original.3, "unknown");
    cancel.cancel();
    let (result, mut original_owner) = tokio::time::timeout(Duration::from_secs(10), serving)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result, Ok(()));
    assert_eq!(original_owner.close().await, Ok(()));
    assert_eq!(f.pending(), original); // cancellation cannot acknowledge sent bytes
    f.reopen();
    f.fake.no_request().await;
    drop(held);
    drop(matrix_or_work);
    drop(other_lane);
    f.fake.close().await;
}
