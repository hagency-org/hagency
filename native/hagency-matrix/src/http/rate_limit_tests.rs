use super::*;
use crate::collector::fixtures as common;
use serde_json::json;
use std::time::Duration;

async fn client(budget: Duration) -> (common::Fixture, common::Fake, Http) {
    let fake = common::Fake::start(true).await;
    let fixture = common::Fixture::new();
    let mut config = fixture
        .config(&fake.endpoint)
        .with_root_pem(include_bytes!("../../tests/fixtures/ca.pem"))
        .unwrap();
    config.limits.request = budget;
    config.limits.headers = config.limits.headers.min(budget);
    config.limits.connect = config.limits.connect.min(budget);
    config.limits.body_idle = config.limits.body_idle.min(budget);
    let http = Http::new(&config).unwrap();
    (fixture, fake, http)
}
fn limited(header: Option<&str>, body: Value) -> Vec<u8> {
    let body = serde_json::to_vec(&body).unwrap();
    let header = header
        .map(|value| format!("Retry-After: {value}\r\n"))
        .unwrap_or_default();
    let mut bytes=format!("HTTP/1.1 429 Fixture\r\nContent-Type: application/json\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
    bytes.extend(body);
    bytes
}

#[tokio::test]
async fn native_matrix_get_rate_limit() {
    for (header, body, delay) in [
        (None, json!({"retry_after_ms":30}), 30),
        (Some("1"), json!({"retry_after_ms":20}), 1000),
        (Some("0"), json!({"retry_after_ms":30}), 30),
        (None, json!({"errcode":"M_LIMIT_EXCEEDED"}), 1000),
    ] {
        let (fixture, mut fake, http) = client(Duration::from_secs(3)).await;
        let cancel = CancellationToken::new();
        let script = async {
            let first = fake.next().await;
            let target = first.target.clone();
            let auth = first.headers["authorization"].clone();
            assert_eq!(first.method, "GET");
            let sent = Instant::now();
            first.raw(limited(header, body));
            let second = fake.next().await;
            assert!(sent.elapsed() >= Duration::from_millis(delay));
            assert_eq!(second.method, "GET");
            assert_eq!(second.target, target);
            assert_eq!(second.headers["authorization"], auth);
            second.json(200, json!({"fresh":true}));
        };
        let (result, ()) = tokio::join!(
            http.request(
                &["_matrix", "client", "v3", "sync"],
                Some(&[("since", "original-cursor")]),
                &cancel
            ),
            script
        );
        assert_eq!(result.unwrap().success().unwrap(), json!({"fresh":true}));
        assert_eq!(fake.requests(), 2);
        fixture.store.shutdown().await.unwrap();
        fake.close().await;
    }
}

#[tokio::test]
async fn native_matrix_get_rate_limit_bounds() {
    for (header, body) in [
        (None, json!({"retry_after_ms":900})),
        (None, json!({"retry_after_ms":-1})),
        (None, json!({"retry_after_ms":"100"})),
        (Some("invalid"), json!({})),
        (Some("18446744073709551616"), json!({})),
        (Some("18446744073709551615"), json!({})),
        (Some("1\r\nRetry-After: 2"), json!({})),
    ] {
        let (fixture, mut fake, http) = client(Duration::from_millis(900)).await;
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(http.request(&["bounded"], None, &cancel), async {
            fake.next().await.raw(limited(header, body));
        });
        assert_eq!(result.unwrap().success(), Err(Error::Remote(429)));
        assert_eq!(fake.requests(), 1);
        fake.no_request().await;
        fixture.store.shutdown().await.unwrap();
        fake.close().await;
    }
    let (fixture, mut fake, http) = client(Duration::from_millis(900)).await;
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(http.request(&["bounded"], None, &cancel), async {
        for _ in 0..4 {
            fake.next().await.json(429, json!({"retry_after_ms":0}));
        }
    });
    assert_eq!(result.unwrap().success(), Err(Error::Remote(429)));
    assert_eq!(fake.requests(), 4);
    fake.no_request().await;
    fixture.store.shutdown().await.unwrap();
    fake.close().await;

    // A fresh deadline per attempt would allow more than these two requests.
    let (fixture, mut fake, http) = client(Duration::from_millis(300)).await;
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(http.request(&["bounded"], None, &cancel), async {
        for _ in 0..2 {
            fake.next().await.json(429, json!({"retry_after_ms":150}));
        }
    });
    assert_eq!(result.unwrap().success(), Err(Error::Remote(429)));
    assert_eq!(fake.requests(), 2);
    fake.no_request().await;
    fixture.store.shutdown().await.unwrap();
    fake.close().await;

    let (fixture, mut fake, http) = client(Duration::from_secs(3)).await;
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(http.request(&["cancel"], None, &cancel), async {
        fake.next().await.json(429, json!({"retry_after_ms":2000}));
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(fake.requests(), 1);
    fake.no_request().await;
    fixture.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_rate_limit_no_write_retry() {
    for method in [reqwest::Method::POST, reqwest::Method::PUT] {
        let (fixture, mut fake, http) = client(Duration::from_secs(3)).await;
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(
            async {
                if method == reqwest::Method::POST {
                    http.post(&["single"], "{}".into(), &cancel).await
                } else {
                    http.put(&["single"], "{}".into(), &cancel).await
                }
            },
            async {
                let request = fake.next().await;
                assert_eq!(request.method, method.as_str());
                request.json(429, json!({"retry_after_ms":0}));
            }
        );
        assert_eq!(result.unwrap().success(), Err(Error::Remote(429)));
        assert_eq!(fake.requests(), 1);
        fake.no_request().await;
        fixture.store.shutdown().await.unwrap();
        fake.close().await;
    }
    let (fixture, mut fake, http) = client(Duration::from_secs(3)).await;
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(http.request(&["lost"], None, &cancel), async {
        fake.next().await.unclean(Vec::new());
    });
    assert!(matches!(result, Err(Error::Transport)));
    assert_eq!(fake.requests(), 1);
    fake.no_request().await;
    fixture.store.shutdown().await.unwrap();
    fake.close().await;
}
