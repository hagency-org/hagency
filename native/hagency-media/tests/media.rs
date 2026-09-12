use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use cap_std::{ambient_authority, fs::Dir};
use hagency_files::{Limits as FileLimits, RelativeFile, Snapshot, Workspace};
use hagency_media::{Codec, Descriptor, Error, Limits};
use matrix_sdk_crypto::{AttachmentDecryptor, MediaEncryptionInfo};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read},
};

fn vectors() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("../../fixtures/media.json")).unwrap()["vectors"]
        .as_array()
        .unwrap()
        .clone()
}
fn fixture(bytes: &[u8], count: usize) -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("报告.bin"), bytes).unwrap();
    let workspace = Workspace::from_directory(
        Dir::open_ambient_dir(dir.path(), ambient_authority()).unwrap(),
        FileLimits::new(1024 * 1024, count).unwrap(),
    )
    .unwrap();
    (dir, workspace)
}
fn snapshot(workspace: &Workspace) -> Snapshot {
    workspace
        .snapshot(&RelativeFile::new("报告.bin").unwrap())
        .unwrap()
}
fn descriptor(value: &Value) -> Descriptor {
    Descriptor::from_private_event_json(&serde_json::to_vec(value).unwrap()).unwrap()
}

#[test]
fn native_media_interoperability() {
    let codec = Codec::new(Limits::default());
    for row in vectors() {
        let ciphertext = STANDARD
            .decode(row["ciphertext"].as_str().unwrap())
            .unwrap();
        let expected: Vec<u8> = (0..row["length"].as_u64().unwrap())
            .map(|i| ((i * 31 + 17) % 256) as u8)
            .collect();
        let result = codec
            .decrypt(&descriptor(&row["descriptor"]), &ciphertext)
            .unwrap();
        assert_eq!(result.bytes(), expected);
        assert_eq!(&result.digest()[..], Sha256::digest(&expected).as_slice());

        let (_dir, workspace) = fixture(&expected, 1);
        let encrypted = codec.encrypt(snapshot(&workspace)).unwrap();
        assert_eq!(encrypted.ciphertext().len(), expected.len());
        // CTR may preserve a byte when its corresponding keystream byte is
        // zero. Ciphertext inequality is not an encryption invariant.
        let metadata: Value =
            serde_json::from_slice(encrypted.descriptor().private_event_json()).unwrap();
        assert_eq!(
            metadata["hashes"]["sha256"],
            STANDARD
                .encode(Sha256::digest(encrypted.ciphertext()))
                .trim_end_matches('=')
        );
        let info: MediaEncryptionInfo =
            serde_json::from_slice(encrypted.descriptor().private_event_json()).unwrap();
        let mut input = Cursor::new(encrypted.ciphertext());
        let mut sdk = AttachmentDecryptor::new(&mut input, info).unwrap();
        let mut actual = Vec::new();
        sdk.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, expected);
        let roundtrip = codec
            .decrypt(encrypted.descriptor(), encrypted.ciphertext())
            .unwrap();
        assert_eq!(roundtrip.bytes(), expected);
    }
    let (_dir, workspace) = fixture("文件内容 🐒\n".as_bytes(), 2);
    let first = codec.encrypt(snapshot(&workspace)).unwrap();
    let second = codec.encrypt(snapshot(&workspace)).unwrap();
    let first_metadata: Value =
        serde_json::from_slice(first.descriptor().private_event_json()).unwrap();
    let second_metadata: Value =
        serde_json::from_slice(second.descriptor().private_event_json()).unwrap();
    assert_ne!(first_metadata["key"]["k"], second_metadata["key"]["k"]);
    assert_ne!(first_metadata["iv"], second_metadata["iv"]);
    let checked = codec
        .decrypt(first.descriptor(), first.ciphertext())
        .unwrap();
    assert_eq!(checked.bytes(), "文件内容 🐒\n".as_bytes());
}

#[test]
fn native_media_ctr_equal_plaintext_vector() {
    // Public Node/OpenSSL AES256-CTR vector: key is 30 zero bytes then 0x016a,
    // IV is all zero, plaintext/ciphertext is 0x11. AES(key, IV) starts with
    // zero, so CTR legitimately preserves this byte. No production randomness
    // is overridden, and the actual SDK must still decrypt the fixed vector.
    let metadata = json!({
        "v": "v2",
        "key": {
            "kty": "oct", "alg": "A256CTR", "ext": true,
            "k": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWo",
            "key_ops": ["encrypt", "decrypt"]
        },
        "iv": "AAAAAAAAAAAAAAAAAAAAAA",
        "hashes": {"sha256": "SmShB/DLMlNuW85smMOT2yHMp/TqGHuoxNyotR1OqAo"}
    });
    let plaintext = [17u8];
    let ciphertext = STANDARD.decode("EQ==").unwrap();
    assert_eq!(ciphertext, plaintext);
    let descriptor = descriptor(&metadata);
    let codec = Codec::new(Limits::default());
    let checked = codec.decrypt(&descriptor, &ciphertext).unwrap();
    assert_eq!(checked.bytes(), plaintext);
    assert_eq!(&checked.digest()[..], Sha256::digest(plaintext).as_slice());
    let info: MediaEncryptionInfo = serde_json::from_value(metadata).unwrap();
    let mut input = Cursor::new(&ciphertext);
    let mut sdk = AttachmentDecryptor::new(&mut input, info).unwrap();
    let mut actual = Vec::new();
    sdk.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, plaintext);
    assert!(matches!(
        codec.decrypt(&descriptor, &[ciphertext[0] ^ 1]),
        Err(Error::Integrity)
    ));
}

#[test]
fn native_media_integrity() {
    let row = &vectors()[5];
    let original = STANDARD
        .decode(row["ciphertext"].as_str().unwrap())
        .unwrap();
    let info = descriptor(&row["descriptor"]);
    let codec = Codec::new(Limits::new(original.len(), 1).unwrap());
    let mut corrupt = original.clone();
    corrupt[0] ^= 1;
    assert!(matches!(
        codec.decrypt(&info, &corrupt),
        Err(Error::Integrity)
    ));
    assert!(matches!(
        codec.decrypt(&info, &original[..original.len() - 1]),
        Err(Error::Integrity)
    ));
    let mut extra = original.clone();
    extra.push(0);
    assert!(matches!(codec.decrypt(&info, &extra), Err(Error::Capacity)));
    let generous = Codec::new(Limits::default());
    assert!(matches!(
        generous.decrypt(&info, &extra),
        Err(Error::Integrity)
    ));
    let checked = codec.decrypt(&info, &original).unwrap(); // failures freed permits
    assert_eq!(checked.bytes().len(), original.len());
    drop(checked);

    // AES-CTR's ciphertext hash does NOT authenticate the key. The surrounding
    // encrypted event must supply provenance; do not claim this can detect it.
    let mut changed = row["descriptor"].clone();
    changed["key"]["k"] = json!(URL_SAFE_NO_PAD.encode([99u8; 32]));
    let different = codec.decrypt(&descriptor(&changed), &original).unwrap();
    let expected: Vec<u8> = (0..original.len())
        .map(|i| ((i * 31 + 17) % 256) as u8)
        .collect();
    assert_ne!(different.bytes(), expected);
}

#[test]
fn native_media_descriptors() {
    let valid = vectors()[1]["descriptor"].clone();
    let raw = serde_json::to_string(&valid).unwrap();
    let exact = format!("{raw}{}", " ".repeat(1024 - raw.len()));
    assert_eq!(exact.len(), 1024);
    assert!(Descriptor::from_private_event_json(exact.as_bytes()).is_ok());
    assert!(matches!(
        Descriptor::from_private_event_json(format!("{exact} ").as_bytes()),
        Err(Error::Descriptor)
    ));
    // Padding and nonzero unused trailing bits may decode to the same key with
    // permissive decoders; the declared canonical encoding must reject them.
    let key = valid["key"]["k"].as_str().unwrap();
    let mut noncanonical = key.as_bytes().to_vec();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let last = alphabet
        .iter()
        .position(|byte| Some(byte) == noncanonical.last())
        .unwrap();
    *noncanonical.last_mut().unwrap() = alphabet[last + 1];
    for encoding in [format!("{key}="), String::from_utf8(noncanonical).unwrap()] {
        let mut doc = valid.clone();
        doc["key"]["k"] = json!(encoding);
        assert!(matches!(
            Descriptor::from_private_event_json(&serde_json::to_vec(&doc).unwrap()),
            Err(Error::Descriptor)
        ));
    }
    let duplicates = [
        raw.replacen("\"v\":\"v2\"", "\"v\":\"v2\",\"v\":\"v2\"", 1),
        raw.replacen("\"ext\":true", "\"ext\":true,\"ext\":true", 1),
        raw.replacen("\"hashes\":{", "\"hashes\":{\"sha256\":\"bad\",", 1),
    ];
    for text in duplicates
        .into_iter()
        .chain([" ".repeat(1025), "[]".to_owned(), "null".to_owned()])
    {
        assert!(matches!(
            Descriptor::from_private_event_json(text.as_bytes()),
            Err(Error::Descriptor)
        ));
    }
    for (pointer, value) in [
        ("/v", json!("v1")),
        ("/key/kty", json!("RSA")),
        ("/key/alg", json!("A128CTR")),
        ("/key/ext", json!(false)),
        ("/key/k", json!("AA")),
        ("/key/key_ops", json!(["decrypt"])),
        ("/key/key_ops", json!(["decrypt", "decrypt"])),
        ("/iv", json!("AAAAAAAAAAAAAAAAAAAAAQ")),
        ("/iv", json!("AA")),
        ("/hashes/sha256", json!("AA")),
        ("/hashes/sha256", json!("private-key-invalid-base64!")),
    ] {
        let mut value_doc = valid.clone();
        *value_doc.pointer_mut(pointer).unwrap() = value;
        assert!(
            matches!(
                Descriptor::from_private_event_json(&serde_json::to_vec(&value_doc).unwrap()),
                Err(Error::Descriptor)
            ),
            "{pointer}"
        );
    }
    for pointer in ["", "/key", "/hashes"] {
        let mut doc = valid.clone();
        doc.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!("secret"));
        assert!(matches!(
            Descriptor::from_private_event_json(&serde_json::to_vec(&doc).unwrap()),
            Err(Error::Descriptor)
        ));
    }
    let error = Descriptor::from_private_event_json(b"private secret bytes")
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "invalid or unsupported attachment descriptor"
    );
    assert!(!format!("{error:?}").contains("private"));
}

#[test]
fn native_media_custody() {
    assert!(Limits::new(0, 1).is_err());
    assert!(Limits::new(16 * 1024 * 1024 + 1, 1).is_err());
    assert!(Limits::new(1, 0).is_err());
    assert!(Limits::new(1, 9).is_err());
    let (dir, workspace) = fixture(b"original", 1);
    let codec = Codec::new(Limits::new(8, 1).unwrap());
    let encrypted = codec.encrypt(snapshot(&workspace)).unwrap();
    assert!(matches!(
        workspace.snapshot(&RelativeFile::new("报告.bin").unwrap()),
        Err(hagency_files::Error::Capacity)
    ));
    let ciphertext = encrypted.ciphertext().to_vec();
    fs::write(dir.path().join("报告.bin"), b"replaced").unwrap();
    assert_eq!(encrypted.ciphertext(), ciphertext);
    assert!(matches!(
        codec.clone().decrypt(encrypted.descriptor(), &ciphertext),
        Err(Error::Capacity)
    ));
    let descriptor =
        Descriptor::from_private_event_json(encrypted.descriptor().private_event_json()).unwrap();
    drop(encrypted);
    let checked = codec.decrypt(&descriptor, &ciphertext).unwrap();
    assert_eq!(checked.bytes(), b"original");
    assert!(matches!(
        codec.encrypt(snapshot(&workspace)),
        Err(Error::Capacity)
    ));
    drop(checked);
    let next = codec.encrypt(snapshot(&workspace)).unwrap();
    let other = Codec::new(Limits::default())
        .decrypt(next.descriptor(), next.ciphertext())
        .unwrap();
    assert_eq!(other.bytes(), b"replaced");
    drop(workspace);
    drop(codec);
    assert_eq!(other.bytes(), b"replaced");

    // All workers retain their result through join; exactly two may own codec
    // storage even when cloned owners compete at the admission boundary.
    let shared = Codec::new(Limits::new(8, 2).unwrap());
    let gate = std::sync::Arc::new(std::sync::Barrier::new(6));
    let info = next.descriptor().private_event_json();
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..6)
            .map(|_| {
                let codec = shared.clone();
                let gate = gate.clone();
                let encrypted = &next;
                scope.spawn(move || {
                    let descriptor = Descriptor::from_private_event_json(info).unwrap();
                    gate.wait();
                    codec.decrypt(&descriptor, encrypted.ciphertext())
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 2);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::Capacity)))
            .count(),
        4
    );
    drop(results);
    assert!(shared.decrypt(next.descriptor(), next.ciphertext()).is_ok());
}
