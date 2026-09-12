mod common;
use common::{Fake, TOKEN};
use hagency_core::replies::{MatrixTransportObservation, RoomPrivacy};
use hagency_matrix::{
    CancellationToken, Error, HostConfig, HostIdentity, HostRoom, MediaUploadError as Failure,
    MediaUploadLimits, MediaUploader, UploadResponse, UploadState,
};
use hagency_media::{Codec, Encrypted};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;

fn config(endpoint: &str) -> HostConfig {
    HostConfig::new(
        HostIdentity {
            server_name: "example.test".into(),
            registration_fingerprint: "a".repeat(64),
            transport: MatrixTransportObservation {
                engagement_id: "host-transfer-only".into(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE_1".into(),
            },
        },
        endpoint,
        TOKEN,
        std::path::PathBuf::from("unused-media-upload-sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: "!direct:example.test".into(),
            generation: 1,
            privacy: RoomPrivacy::Direct {
                human_mxid: "@owner:example.test".into(),
            },
        }],
        common::limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("fixtures/ca.pem"))
    .unwrap()
}
fn uploader(fake: &Fake, bytes: usize, active: usize, attempts: usize) -> MediaUploader {
    MediaUploader::new(
        &config(&fake.endpoint),
        MediaUploadLimits::new(bytes, active, attempts).unwrap(),
    )
    .unwrap()
}
struct Media {
    encrypted: Encrypted,
    _root: tempfile::TempDir,
}
fn media(plaintext: &[u8]) -> Media {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("private-file"), plaintext).unwrap();
    let dir =
        cap_std::fs::Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let workspace =
        hagency_files::Workspace::from_directory(dir, hagency_files::Limits::default()).unwrap();
    let snapshot = workspace
        .snapshot(&hagency_files::RelativeFile::new("private-file").unwrap())
        .unwrap();
    let encrypted = Codec::new(hagency_media::Limits::default())
        .encrypt(snapshot)
        .unwrap();
    Media {
        _root: root,
        encrypted,
    }
}
fn accepted() -> serde_json::Value {
    json!({"content_uri":"mxc://media.remote:8448/Abc_123-XYZ"})
}
fn reply(bytes: &[u8]) -> Vec<u8> {
    common::response(200, bytes)
}
fn unframed(bytes: &[u8]) -> Vec<u8> {
    let mut result =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n".to_vec();
    result.extend_from_slice(bytes);
    result
}

#[tokio::test]
async fn native_matrix_upload_ciphertext_origin() {
    let plaintext = b"Private file text. This exact original must never be uploaded as plaintext.";
    let media = media(plaintext);
    let cipher = media.encrypted.ciphertext().to_vec();
    let descriptor = media.encrypted.descriptor().private_event_json().to_vec();
    let mut fake = Fake::start(true).await;
    let client = uploader(&fake, 1024, 1, 1);
    let mut attempt = client.prepare(&media.encrypted).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(attempt.state(), UploadState::Prepared);
    let (result, ()) = tokio::join!(attempt.send(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/_matrix/media/v3/upload");
        assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
        assert_eq!(request.headers["content-type"], "application/octet-stream");
        assert_eq!(request.headers["content-length"], cipher.len().to_string());
        assert_eq!(request.body, cipher);
        assert_ne!(request.body, plaintext);
        assert!(
            !request
                .body
                .windows(descriptor.len())
                .any(|v| v == descriptor)
        );
        request.json(200, accepted());
    });
    result.unwrap();
    assert_eq!(attempt.state(), UploadState::Accepted);
    assert_eq!(
        attempt.media_id().unwrap().to_mxc(),
        "mxc://media.remote:8448/Abc_123-XYZ"
    );
    assert_eq!(attempt.failure(), None);
    assert_eq!(attempt.send(&cancel).await, Err(Failure::Terminal));
    fake.no_request().await;
    assert_eq!(media.encrypted.ciphertext(), cipher);
    assert_eq!(
        media.encrypted.descriptor().private_event_json(),
        descriptor
    );
    drop(attempt);
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_response_bounds() {
    let valid = serde_json::to_vec(&accepted()).unwrap();
    let mut too_large = valid.clone();
    too_large.resize(4097, b' ');
    let framed = |headers: &str| {
        let mut bytes =
            format!("HTTP/1.1 200 OK\r\n{headers}Connection: close\r\n\r\n").into_bytes();
        bytes.extend_from_slice(&valid);
        bytes
    };
    let mut invalid = vec![
        reply(br#"{"content_uri":"mxc://a/a","content_uri":"mxc://b/b"}"#),
        reply(br#"{"content_uri":"mxc://a/a","extra":1}"#),
        reply(br#"{"content_uri":1}"#),
        reply(b"null"),
        reply(b"[]"),
        reply(br#"{"content_uri":"mxc://a/a"}{}"#),
        reply(&too_large),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4097\r\n\r\n"
            .to_vec(),
        framed(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            valid.len() + 1
        )),
        framed(&format!(
            "Content-Type: application/json\r\nContent-Length: {0}\r\nContent-Length: {0}\r\n",
            valid.len()
        )),
        framed("Content-Type: application/json\r\nContent-Encoding: gzip\r\n"),
        framed("Content-Type: application/json\r\nContent-Type: application/json\r\n"),
        framed("Content-Type: text/plain\r\n"),
        framed(&format!(
            "Content-Type: application/json\r\nX-Oversized: {}\r\n",
            "a".repeat(16384)
        )),
    ];
    let mut ambiguous = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",valid.len(),valid.len()).into_bytes();
    ambiguous.extend_from_slice(&valid);
    ambiguous.extend_from_slice(b"\r\n0\r\n\r\n");
    invalid.push(ambiguous);
    let mut truncated_chunks = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",valid.len()).into_bytes();
    truncated_chunks.extend_from_slice(&valid);
    truncated_chunks.extend_from_slice(b"\r\n"); // Missing final zero chunk.
    invalid.push(truncated_chunks);
    for uri in [
        "https://evil.test/a",
        "mxc://a/",
        "mxc://a/..",
        "mxc://a/%2e",
        "mxc://a/a?b",
        "mxc://a/a#b",
        "mxc://a/a/b",
        "mxc://user@a/a",
    ] {
        invalid.push(reply(
            &serde_json::to_vec(&json!({"content_uri":uri})).unwrap(),
        ));
    }
    let nested = format!(
        "{{\"content_uri\":{}0{}}}",
        "[".repeat(130),
        "]".repeat(130)
    );
    invalid.push(reply(nested.as_bytes()));
    let media = media(b"bounded encrypted data");
    let mut fake = Fake::start(true).await;
    let client = uploader(&fake, 1024, 1, 1);
    for bytes in invalid {
        let mut attempt = client.prepare(&media.encrypted).unwrap();
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(attempt.send(&cancel), async {
            fake.next().await.raw(bytes);
        });
        assert!(result.is_err());
        assert_eq!(attempt.state(), UploadState::WritePossible);
        assert!(attempt.media_id().is_none());
        assert!(attempt.observed_response().is_none());
        assert!(attempt.failure().is_some());
        assert_eq!(attempt.send(&cancel).await, Err(Failure::Terminal));
    }
    // Without Content-Length, unclean TLS EOF cannot prove the response ended.
    let mut attempt = client.prepare(&media.encrypted).unwrap();
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(attempt.send(&cancel), async {
        fake.next().await.unclean(unframed(&valid));
    });
    assert_eq!(result, Err(Failure::Transport(Error::Transport)));
    assert_eq!(attempt.state(), UploadState::WritePossible);
    assert!(attempt.observed_response().is_none());
    drop(attempt);
    // Exactly4096 bytes with valid whitespace is accepted, demonstrating the cap.
    let mut exact = valid;
    exact.resize(4096, b' ');
    let mut attempt = client.prepare(&media.encrypted).unwrap();
    let (result, ()) = tokio::join!(attempt.send(&cancel), async {
        fake.next().await.raw(reply(&exact));
    });
    result.unwrap();
    assert_eq!(attempt.state(), UploadState::Accepted);
    assert_eq!(attempt.observed_response().unwrap().body(), exact);
    assert_eq!(
        attempt
            .observed_response()
            .unwrap()
            .body_sha256()
            .as_slice(),
        Sha256::digest(&exact).as_slice()
    );
    drop(attempt);
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_cancellation_custody() {
    let media = media(b"retained original bytes and descriptor across failed HTTP");
    let ciphertext = media.encrypted.ciphertext().to_vec();
    let descriptor = media.encrypted.descriptor().private_event_json().to_vec();
    let mut fake = Fake::start(true).await;
    let client = uploader(&fake, 1024, 1, 1);
    for mode in ["cancel", "drop", "header_timeout", "body_idle", "absolute"] {
        let mut attempt = client.prepare(&media.encrypted).unwrap();
        let cancel = CancellationToken::new();
        let mut sending = Box::pin(attempt.send(&cancel));
        let request = tokio::select! {
            result=&mut sending=>panic!("send completed before peer read: {result:?}"),
            request=fake.next()=>request,
        };
        assert_eq!(request.body, ciphertext);
        match mode {
            "cancel" => {
                cancel.cancel();
                assert_eq!(sending.await, Err(Failure::Transport(Error::Cancelled)));
                drop(request);
            }
            "drop" => {
                drop(sending);
                drop(request);
            }
            "header_timeout" => {
                assert_eq!(sending.await, Err(Failure::Transport(Error::Timeout)));
                drop(request);
            }
            "body_idle" => {
                request.chunks(vec![
                    (Duration::ZERO, b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 99\r\n\r\n{".to_vec()),
                    (Duration::from_millis(400), b"}".to_vec()),
                ]);
                assert_eq!(sending.await, Err(Failure::Transport(Error::Timeout)));
            }
            "absolute" => {
                let body = serde_json::to_vec(&accepted()).unwrap();
                let mut pieces = vec![(Duration::ZERO, format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes())];
                pieces.extend(
                    body.into_iter()
                        .map(|byte| (Duration::from_millis(100), vec![byte])),
                );
                request.chunks(pieces); // Every interval is below the idle bound.
                assert_eq!(sending.await, Err(Failure::Transport(Error::Timeout)));
            }
            _ => unreachable!(),
        }
        assert_eq!(attempt.state(), UploadState::WritePossible);
        assert!(attempt.media_id().is_none());
        assert!(attempt.observed_response().is_none());
        assert_eq!(
            attempt.send(&CancellationToken::new()).await,
            Err(Failure::Terminal)
        );
        fake.no_request().await;
        assert_eq!(media.encrypted.ciphertext(), ciphertext);
        assert_eq!(
            media.encrypted.descriptor().private_event_json(),
            descriptor
        );
    }
    let mut attempt = client.prepare(&media.encrypted).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        attempt.send(&cancel).await,
        Err(Failure::Transport(Error::Cancelled))
    );
    assert_eq!(attempt.state(), UploadState::Prepared); // Nothing was polled.
    fake.no_request().await;
    drop(attempt);
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_capacity() {
    let media = media(b"shared capacity");
    let mut fake = Fake::start(true).await;
    let client = uploader(&fake, 1024, 1, 2);
    let clone = client.clone();
    let mut first = client.prepare(&media.encrypted).unwrap();
    let mut second = clone.prepare(&media.encrypted).unwrap();
    assert!(matches!(
        client.prepare(&media.encrypted),
        Err(Failure::Transport(Error::Busy))
    ));
    let cancel = CancellationToken::new();
    let mut sending = Box::pin(first.send(&cancel));
    let request = tokio::select! {
        result=&mut sending=>panic!("send completed before peer read: {result:?}"),
        request=fake.next()=>request,
    };
    assert_eq!(
        second.send(&cancel).await,
        Err(Failure::Transport(Error::Busy))
    );
    assert_eq!(second.state(), UploadState::Prepared);
    drop(sending);
    drop(request);
    assert_eq!(first.state(), UploadState::WritePossible);
    let (result, ()) = tokio::join!(second.send(&cancel), async {
        fake.next().await.json(200, accepted());
    });
    result.unwrap();
    assert_eq!(second.state(), UploadState::Accepted);
    // Unknown and accepted receipts continue owning both finite slots.
    assert!(matches!(
        clone.prepare(&media.encrypted),
        Err(Failure::Transport(Error::Busy))
    ));
    drop(first);
    let third = clone.prepare(&media.encrypted).unwrap();
    drop(third);
    drop(second);
    let tiny = uploader(&fake, 1, 1, 1);
    assert!(matches!(
        tiny.prepare(&media.encrypted),
        Err(Failure::Transport(Error::BodyTooLarge))
    ));
    fake.no_request().await;
    for (bytes, active, held) in [
        (0, 1, 1),
        (16 * 1024 * 1024 + 1, 1, 1),
        (1, 0, 1),
        (1, 5, 1),
        (1, 1, 0),
        (1, 1, 9),
    ] {
        assert!(matches!(
            MediaUploadLimits::new(bytes, active, held),
            Err(Failure::Config)
        ));
    }
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_refusals() {
    let media = media(b"no unsafe retries");
    let mut alternate = Fake::start(true).await;
    let mut fake = Fake::start(true).await;
    let client = uploader(&fake, 1024, 1, 1);
    for status in [301, 401, 403, 429, 500] {
        let mut attempt = client.prepare(&media.encrypted).unwrap();
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(attempt.send(&cancel), async {
            let request = fake.next().await;
            if status == 301 {
                request.raw(
                    format!(
                        "HTTP/1.1 301 Moved\r\nLocation: {}\r\nContent-Length: 0\r\n\r\n",
                        alternate.endpoint
                    )
                    .into_bytes(),
                );
            } else {
                request.json(status, json!({"errcode":"M_FORBIDDEN"}));
            }
        });
        assert!(result.is_err());
        assert_eq!(attempt.state(), UploadState::WritePossible);
        assert_eq!(attempt.send(&cancel).await, Err(Failure::Terminal));
    }
    alternate.no_request().await;
    let unavailable = Fake::start(true).await;
    let unreachable = uploader(&unavailable, 1024, 1, 1);
    unavailable.close().await;
    let mut attempt = unreachable.prepare(&media.encrypted).unwrap();
    assert!(attempt.send(&CancellationToken::new()).await.is_err());
    assert_eq!(attempt.state(), UploadState::WritePossible);
    drop(attempt);
    let plain = Fake::start(false).await;
    assert!(matches!(
        MediaUploader::new(&config(&plain.endpoint), MediaUploadLimits::default()),
        Err(Failure::Config)
    ));
    plain.close().await;
    fake.close().await;
    alternate.close().await;
}

#[tokio::test]
async fn native_matrix_upload_response_evidence() {
    trait Sealed<A> {
        fn check() {}
    }
    impl<T: ?Sized> Sealed<()> for T {}
    impl<T: Clone> Sealed<u8> for T {}
    impl<T: std::fmt::Debug> Sealed<u16> for T {}
    impl<T: serde::Serialize> Sealed<u32> for T {}
    impl<T: serde::de::DeserializeOwned> Sealed<u64> for T {}
    let _ = <UploadResponse as Sealed<_>>::check;
    let media = media(b"original encrypted input");
    let mut fake = Fake::start(true).await;
    let client = uploader(&fake, 1024, 1, 1);
    let bodies: [&[u8]; 3] = [
        br#"{"content_uri":"mxc://media.remote:8448/Abc_123-XYZ"}"#,
        b" \n { \"content_uri\" : \"mxc://media.remote:8448/Abc_123-XYZ\" } \t ",
        br#"{"content_uri":"mxc:\/\/media.remote:8448/\u0041bc_123-XYZ"}"#,
    ];
    let mut digests = std::collections::BTreeSet::new();
    for raw in bodies {
        let mut attempt = client.prepare(&media.encrypted).unwrap();
        assert!(attempt.observed_response().is_none());
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(attempt.send(&cancel), async {
            fake.next().await.raw(reply(raw));
        });
        result.unwrap();
        let observed = attempt.observed_response().unwrap();
        assert_eq!(observed.body(), raw);
        let expected: [u8; 32] = Sha256::digest(raw).into();
        assert_eq!(observed.body_sha256(), &expected);
        assert!(digests.insert(*observed.body_sha256()));
        assert_eq!(
            observed.media_id().to_mxc(),
            "mxc://media.remote:8448/Abc_123-XYZ"
        );
        assert_eq!(
            attempt.media_id().unwrap().to_mxc(),
            observed.media_id().to_mxc()
        );
        assert!(matches!(
            client.prepare(&media.encrypted),
            Err(Failure::Transport(Error::Busy))
        ));
        assert_eq!(attempt.send(&cancel).await, Err(Failure::Terminal));
        // The original response remains held and unchanged after refused resend.
        assert_eq!(attempt.observed_response().unwrap().body(), raw);
        drop(attempt);
    }
    fake.close().await;
}
