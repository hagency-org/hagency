mod common;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{Fake, TOKEN};
use hagency_core::replies::{MatrixTransportObservation, RoomPrivacy};
use hagency_matrix::{
    CancellationToken, Error, HostConfig, HostIdentity, HostRoom, Limits,
    MediaDownloadError as Failure, MediaDownloadLimits, MediaDownloader, MediaId,
};
use hagency_media::{CheckedBytes, Descriptor};
use serde_json::Value;
use std::time::Duration;
use tokio::time::{Instant, timeout};

fn config(endpoint: &str, limits: Limits) -> HostConfig {
    HostConfig::new(
        HostIdentity {
            server_name: "example.test".into(),
            registration_fingerprint: "a".repeat(64),
            transport: MatrixTransportObservation {
                engagement_id: "host-storage-observation-only".into(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE_1".into(),
            },
        },
        endpoint,
        TOKEN,
        // This transport must never open an SDK or create state at this path.
        std::path::PathBuf::from("unused-media-download-sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: "!direct:example.test".into(),
            generation: 1,
            privacy: RoomPrivacy::Direct {
                human_mxid: "@owner:example.test".into(),
            },
        }],
        limits,
    )
    .unwrap()
}
fn client(fake: &Fake, bytes: usize, active: usize, results: usize) -> MediaDownloader {
    configured(fake, common::limits(), bytes, active, results)
}
fn configured(
    fake: &Fake,
    timing: Limits,
    bytes: usize,
    active: usize,
    results: usize,
) -> MediaDownloader {
    MediaDownloader::new(
        &config(&fake.endpoint, timing)
            .with_root_pem(include_bytes!("fixtures/ca.pem"))
            .unwrap(),
        MediaDownloadLimits::new(bytes, active, results).unwrap(),
    )
    .unwrap()
}
fn media() -> MediaId {
    MediaId::new("mxc://media.remote:8448/Abc_-123").unwrap()
}
fn vector(len: usize) -> (Vec<u8>, Descriptor, Vec<u8>) {
    let all: Value = serde_json::from_str(include_str!("../../fixtures/media.json")).unwrap();
    let row = all["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["length"] == len)
        .unwrap();
    let encrypted = STANDARD
        .decode(row["ciphertext"].as_str().unwrap())
        .unwrap();
    let descriptor =
        Descriptor::from_private_event_json(&serde_json::to_vec(&row["descriptor"]).unwrap())
            .unwrap();
    let expected = (0..len).map(|i| ((i * 31 + 17) % 256) as u8).collect();
    (encrypted, descriptor, expected)
}
fn body(bytes: &[u8]) -> Vec<u8> {
    let mut output = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).into_bytes();
    output.extend_from_slice(bytes);
    output
}
fn chunked(bytes: &[u8], complete: bool) -> Vec<u8> {
    let mut output =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    for chunk in bytes.chunks(7) {
        output.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
        output.extend_from_slice(chunk);
        output.extend_from_slice(b"\r\n");
    }
    if complete {
        output.extend_from_slice(b"0\r\n\r\n");
    }
    output
}
async fn exchange(
    client: &MediaDownloader,
    fake: &mut Fake,
    id: &MediaId,
    descriptor: &Descriptor,
    pieces: Vec<(Duration, Vec<u8>)>,
    cancel: &CancellationToken,
) -> Result<CheckedBytes, Failure> {
    let run = client.download(id, descriptor, cancel);
    tokio::pin!(run);
    let request = tokio::select! {
        biased;
        request = fake.next() => request,
        result = &mut run => match result {
            Err(error) => panic!("download stopped before expected HTTP request: {error}"),
            Ok(_) => panic!("download completed without expected HTTP request"),
        },
    };
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.target,
        "/_matrix/client/v1/media/download/media.remote:8448/Abc_-123"
    );
    assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
    assert_eq!(request.headers["accept-encoding"], "identity");
    assert_eq!(request.headers["accept"], "application/octet-stream");
    assert!(request.body.is_empty());
    request.chunks(pieces);
    run.await
}
fn error(result: Result<CheckedBytes, Failure>) -> Failure {
    match result {
        Err(error) => error,
        Ok(_) => panic!("unexpected checked plaintext"),
    }
}

#[tokio::test]
async fn native_matrix_media_origin() {
    let mut fake = Fake::start(true).await;
    let id = media();
    let cancel = CancellationToken::new();
    let (cipher, descriptor, expected) = vector(16);
    let untrusted = MediaDownloader::new(
        &config(&fake.endpoint, common::limits()),
        MediaDownloadLimits::default(),
    )
    .unwrap();
    assert_eq!(
        error(untrusted.download(&id, &descriptor, &cancel).await),
        Failure::Transport(Error::Transport)
    );
    fake.no_request().await;
    let client = client(&fake, 1024, 1, 1);
    let result = exchange(
        &client,
        &mut fake,
        &id,
        &descriptor,
        vec![(Duration::ZERO, body(&cipher))],
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.bytes(), expected);
    drop(result);
    for (status, failure) in [
        (307, Error::Redirect),
        (308, Error::Redirect),
        (401, Error::Unauthorized),
        (403, Error::Unauthorized),
        (404, Error::Remote(404)),
        (410, Error::Remote(410)),
        (429, Error::Remote(429)),
    ] {
        let response = format!("HTTP/1.1 {status} Failure\r\nLocation: https://secret-target.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes();
        assert_eq!(
            error(
                exchange(
                    &client,
                    &mut fake,
                    &id,
                    &descriptor,
                    vec![(Duration::ZERO, response)],
                    &cancel
                )
                .await
            ),
            Failure::Transport(failure)
        );
        fake.no_request().await;
    }
    fake.close().await;
    assert!(matches!(
        MediaDownloader::new(
            &config("http://127.0.0.1:1234/", common::limits()),
            MediaDownloadLimits::default()
        ),
        Err(Failure::Config)
    ));
}

#[test]
fn native_matrix_media_identifiers() {
    for valid in [
        "mxc://example.test/abc_-123",
        "mxc://remote:8448/a",
        "mxc://[2001:db8::1]:443/ID",
        "mxc://127.0.0.1/a",
    ] {
        assert!(MediaId::new(valid).is_ok());
    }
    for invalid in [
        "",
        "https://example.test/a",
        "MXC://example.test/a",
        "mxc://test/",
        "mxc:///a",
        "mxc://test/a/b",
        "mxc://test/.",
        "mxc://test/..",
        "mxc://./a",
        "mxc://../a",
        "mxc://test/%2e%2e",
        "mxc://test/%2Fsecret",
        "mxc://test/a?b",
        "mxc://test/a#b",
        "mxc://test/a\\b",
        "mxc://user@test/a",
        "mxc://test%2Fother/a",
        "mxc://test\n/a",
        "mxc://test/中文",
        "mxc://[invalid]/a",
        "mxc://test:65536/a",
    ] {
        assert!(matches!(MediaId::new(invalid), Err(Failure::MediaId)));
    }
    // The component parser is explicitly bounded and never calls ruma's u8
    // slash-index conversion for long server names (including its zero wrap).
    for n in [249, 250, 255] {
        assert!(MediaId::new(&format!("mxc://{}/a", "a".repeat(n))).is_ok());
    }
    assert!(MediaId::new(&format!("mxc://{}/a", "a".repeat(256))).is_err());
    assert!(MediaId::new(&format!("mxc://test/{}", "a".repeat(255))).is_ok());
    assert!(MediaId::new(&format!("mxc://test/{}", "a".repeat(256))).is_err());
    for limits in [
        (0, 1, 1),
        (16 * 1024 * 1024 + 1, 1, 1),
        (1, 0, 1),
        (1, 5, 1),
        (1, 1, 0),
        (1, 1, 9),
    ] {
        assert!(matches!(
            MediaDownloadLimits::new(limits.0, limits.1, limits.2),
            Err(Failure::Config)
        ));
    }
}

#[tokio::test]
async fn native_matrix_media_headers_bounds() {
    let mut fake = Fake::start(true).await;
    let client = client(&fake, 16, 1, 1);
    let id = media();
    let cancel = CancellationToken::new();
    let (cipher, descriptor, expected) = vector(16);
    let malformed = [
        "Content-Length: 16\r\nContent-Length: 16\r\n",
        "Content-Length: 16\r\nContent-Length: 17\r\n",
        "Content-Length: 16, 16\r\n",
        "Content-Length: 16\r\nTransfer-Encoding: chunked\r\n",
        "Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n",
        "Transfer-Encoding: gzip, chunked\r\n",
        "Content-Encoding: identity\r\nContent-Encoding: identity\r\n",
        "Content-Encoding: gzip\r\n",
        "Content-Type: a\r\nContent-Type: b\r\n",
        "Content-Length: +16\r\n",
        "Content-Length: 18446744073709551616\r\n",
    ];
    for headers in malformed {
        let mut wire =
            format!("HTTP/1.1 200 OK\r\n{headers}Connection: close\r\n\r\n").into_bytes();
        wire.extend_from_slice(&cipher);
        let failure = error(
            exchange(
                &client,
                &mut fake,
                &id,
                &descriptor,
                vec![(Duration::ZERO, wire)],
                &cancel,
            )
            .await,
        );
        // Hyper refuses conflicting/invalid framing before HeaderMap exists;
        // the binary adapter refuses valid-but-duplicate fields itself.
        assert!(matches!(
            failure,
            Failure::Transport(Error::Headers | Error::Transport)
        ));
    }
    for headers in [
        format!("X-Large: {}\r\n", "a".repeat(16385)),
        (0..65).map(|i| format!("X-{i}: a\r\n")).collect(),
    ] {
        let wire = format!("HTTP/1.1 200 OK\r\n{headers}Content-Length: 0\r\n\r\n").into_bytes();
        assert_eq!(
            error(
                exchange(
                    &client,
                    &mut fake,
                    &id,
                    &descriptor,
                    vec![(Duration::ZERO, wire)],
                    &cancel
                )
                .await
            ),
            Failure::Transport(Error::Headers)
        );
    }
    for wire in [
        body(&[0; 17]),
        chunked(&[0; 17], true),
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n12345678901234567".to_vec(),
    ] {
        assert_eq!(
            error(
                exchange(
                    &client,
                    &mut fake,
                    &id,
                    &descriptor,
                    vec![(Duration::ZERO, wire)],
                    &cancel
                )
                .await
            ),
            Failure::Transport(Error::BodyTooLarge)
        );
    }
    let result = exchange(
        &client,
        &mut fake,
        &id,
        &descriptor,
        vec![(Duration::ZERO, body(&cipher))],
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.bytes(), expected);
    drop(result);
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_media_integrity_eof() {
    let mut fake = Fake::start(true).await;
    let client = client(&fake, 1024, 1, 1);
    let id = media();
    let cancel = CancellationToken::new();
    let (cipher, descriptor, expected) = vector(16);
    let mut close = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    close.extend_from_slice(&cipher);
    {
        let run = client.download(&id, &descriptor, &cancel);
        tokio::pin!(run);
        let request = tokio::select! {
            request=fake.next()=>request,
            _=&mut run=>panic!("download ended before request"),
        };
        request.unclean(close.clone());
        assert_eq!(error(run.await), Failure::Transport(Error::Transport));
    }
    for wire in [chunked(&cipher, true), close] {
        let result = exchange(
            &client,
            &mut fake,
            &id,
            &descriptor,
            vec![(Duration::ZERO, wire)],
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(result.bytes(), expected);
    }
    for wire in [chunked(&cipher, false), {
        let mut wire = body(&cipher);
        wire.pop();
        wire
    }] {
        assert_eq!(
            error(
                exchange(
                    &client,
                    &mut fake,
                    &id,
                    &descriptor,
                    vec![(Duration::ZERO, wire)],
                    &cancel
                )
                .await
            ),
            Failure::Transport(Error::Transport)
        );
    }
    for bad in [
        {
            let mut b = cipher.clone();
            b[0] ^= 1;
            b
        },
        cipher[..15].to_vec(),
        {
            let mut b = cipher.clone();
            b.push(0);
            b
        },
    ] {
        assert_eq!(
            error(
                exchange(
                    &client,
                    &mut fake,
                    &id,
                    &descriptor,
                    vec![(Duration::ZERO, body(&bad))],
                    &cancel
                )
                .await
            ),
            Failure::Crypto(hagency_media::Error::Integrity)
        );
    }
    let (empty, descriptor, _) = vector(0);
    let result = exchange(
        &client,
        &mut fake,
        &id,
        &descriptor,
        vec![(Duration::ZERO, body(&empty))],
        &cancel,
    )
    .await
    .unwrap();
    assert!(result.bytes().is_empty());
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_media_deadline_cancel() {
    let mut fake = Fake::start(true).await;
    let timing = Limits {
        connect: Duration::from_millis(150),
        headers: Duration::from_millis(200),
        request: Duration::from_millis(600),
        body_idle: Duration::from_millis(180),
        ..common::limits()
    };
    let client = configured(&fake, timing, 1024, 1, 1);
    let id = media();
    let (cipher, descriptor, _) = vector(16);
    for pieces in [
        vec![(Duration::from_millis(350), body(&cipher))],
        vec![
            (
                Duration::ZERO,
                b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\n".to_vec(),
            ),
            (Duration::from_millis(350), cipher.clone()),
        ],
        {
            let mut pieces = vec![(
                Duration::ZERO,
                b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\n".to_vec(),
            )];
            pieces.extend(
                cipher
                    .iter()
                    .map(|b| (Duration::from_millis(100), vec![*b])),
            );
            pieces
        },
    ] {
        let cancel = CancellationToken::new();
        let started = Instant::now();
        assert_eq!(
            error(exchange(&client, &mut fake, &id, &descriptor, pieces, &cancel).await),
            Failure::Transport(Error::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        error(client.download(&id, &descriptor, &cancel).await),
        Failure::Transport(Error::Cancelled)
    );
    fake.no_request().await;
    let cancel = CancellationToken::new();
    let run = client.download(&id, &descriptor, &cancel);
    tokio::pin!(run);
    let request = tokio::select! { request=fake.next()=>request, _=&mut run=>panic!("download ended before request") };
    request.chunks(vec![
        (
            Duration::ZERO,
            b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\n".to_vec(),
        ),
        (Duration::from_secs(2), cipher.clone()),
    ]);
    cancel.cancel();
    assert_eq!(error(run.await), Failure::Transport(Error::Cancelled));
    // Drop a polled future after the peer observed its GET; no detached transfer
    // can retain the permit or issue another request after this scope ends.
    {
        let cancel = CancellationToken::new();
        let run = client.download(&id, &descriptor, &cancel);
        tokio::pin!(run);
        let request = tokio::select! { request=fake.next()=>request,_=&mut run=>panic!("download ended before request")};
        request.chunks(vec![(Duration::from_secs(2), body(&cipher))]);
    }
    let cancel = CancellationToken::new();
    let result = exchange(
        &client,
        &mut fake,
        &id,
        &descriptor,
        vec![(Duration::ZERO, body(&cipher))],
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(result.bytes().len(), 16);
    drop(result);
    fake.no_request().await;
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_media_capacity() {
    let mut fake = Fake::start(true).await;
    let client = client(&fake, 1024, 1, 1);
    let other = client.clone();
    let id = media();
    let cancel = CancellationToken::new();
    let (cipher, descriptor, _) = vector(16);
    let run = client.download(&id, &descriptor, &cancel);
    tokio::pin!(run);
    let request = tokio::select! {request=fake.next()=>request,_=&mut run=>panic!("download ended before request")};
    assert_eq!(
        error(other.download(&id, &descriptor, &cancel).await),
        Failure::Transport(Error::Busy)
    );
    fake.no_request().await;
    request.raw(body(&cipher));
    let held = run.await.unwrap();
    // Transfer buffers and completed codec results have separate explicit caps.
    // A full codec refuses another result after its bounded download, never
    // releasing an earlier caller's checked plaintext permit.
    assert_eq!(
        error(
            exchange(
                &other,
                &mut fake,
                &id,
                &descriptor,
                vec![(Duration::ZERO, body(&cipher))],
                &cancel
            )
            .await
        ),
        Failure::Crypto(hagency_media::Error::Capacity)
    );
    assert_eq!(held.bytes().len(), 16);
    drop(held);
    let result = timeout(
        Duration::from_secs(2),
        exchange(
            &client,
            &mut fake,
            &id,
            &descriptor,
            vec![(Duration::ZERO, body(&cipher))],
            &cancel,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.bytes().len(), 16);
    drop(result);
    fake.close().await;
}
