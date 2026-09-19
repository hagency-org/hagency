use super::*;
use hagency_media::Descriptor;
use sha2::{Digest, Sha256};

#[test]
fn native_media_encrypted_receipt_association() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    let encrypted = codec()
        .encrypt(f.snapshot(b"original encrypted receipt"))
        .unwrap();
    let ciphertext = encrypted.ciphertext().to_vec();
    let descriptor =
        Descriptor::from_private_event_json(encrypted.descriptor().private_event_json()).unwrap();
    assert_eq!(
        descriptor.ciphertext_sha256(),
        &<[u8; 32]>::from(Sha256::digest(&ciphertext))
    );
    let operation = op("original");
    let before = journal_bytes(&mut store);
    let plan = store.prepare_encrypted(&operation, encrypted);
    assert_eq!(journal_bytes(&mut store), before);
    assert_sync(store.sync_evidence());
    let receipt = match store.sync_evidence() {
        SyncEvidence::FileAndDirectorySynced => {
            let prepared = plan.map_err(|e| e.error()).unwrap();
            assert!(encrypted_receipt_matches(
                namespace().digest(),
                &operation,
                ciphertext.len(),
                &descriptor,
                prepared.digest()
            ));
            store
                .stage_prepared(prepared)
                .map_err(|e| e.error())
                .unwrap()
        }
        SyncEvidence::FileSyncedDirectoryUnconfirmed => {
            let failure = plan.err().unwrap();
            assert_eq!(failure.error(), Error::Durability);
            assert_eq!(failure.custody(), FailureCustody::ReturnedUnadmitted);
            // An ordinary receipt can be associated without becoming qualified
            // preparation or upload custody on this platform.
            store
                .stage(&operation, failure.into_unadmitted().unwrap())
                .map_err(|e| e.error())
                .unwrap()
        }
    };
    assert!(encrypted_receipt_matches(
        namespace().digest(),
        &operation,
        ciphertext.len(),
        &descriptor,
        receipt.digest()
    ));
    // Compatibility oracle frozen from the pre-ADR100 frame layout. Compare the
    // entire actual first frame, including digest, check and chain, independently
    // of the new shared identity-fields function.
    let mut intent = [0u8; 192];
    let descriptor_bytes = descriptor.private_event_json();
    let length = 192 + operation.as_str().len() + ciphertext.len() + descriptor_bytes.len() + 40;
    intent[..8].copy_from_slice(b"HGMBEGIN");
    intent[8..16].copy_from_slice(&(length as u64).to_le_bytes());
    intent[16..24].copy_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    intent[24..28].copy_from_slice(&(descriptor_bytes.len() as u32).to_le_bytes());
    intent[28..30].copy_from_slice(&(operation.as_str().len() as u16).to_le_bytes());
    intent[30] = 2;
    intent[32..64].copy_from_slice(&Sha256::digest(&ciphertext));
    intent[64..96].copy_from_slice(&Sha256::digest(descriptor_bytes));
    intent[96..128].copy_from_slice(&Sha256::digest(&before));
    let digest = Sha256::digest(
        [
            namespace().digest().as_slice(),
            operation.as_str().as_bytes(),
            &intent[16..32],
            &intent[32..96],
        ]
        .concat(),
    );
    assert_eq!(receipt.digest().as_slice(), digest.as_slice());
    intent[128..160].copy_from_slice(&digest);
    let check = Sha256::digest(
        [
            namespace().digest().as_slice(),
            &intent[..160],
            operation.as_str().as_bytes(),
        ]
        .concat(),
    );
    intent[160..].copy_from_slice(&check);
    let mut frame = [
        &intent,
        operation.as_str().as_bytes(),
        &ciphertext,
        descriptor_bytes,
    ]
    .concat();
    let chain = Sha256::digest(&frame);
    frame.extend_from_slice(b"HGMCOMIT");
    frame.extend_from_slice(&chain);
    assert_eq!(&journal_bytes(&mut store)[before.len()..], &frame);
    drop(store);
    let mut reopened = f.reopen(Limits::default()).unwrap();
    let restored = reopened.read(&operation).unwrap();
    assert_eq!(restored.bytes(), ciphertext);
    assert_eq!(restored.receipt().digest(), receipt.digest());
    assert!(encrypted_receipt_matches(
        namespace().digest(),
        &operation,
        restored.bytes().len(),
        restored.descriptor().unwrap(),
        receipt.digest()
    ));
}

#[test]
fn native_media_encrypted_receipt_substitution() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    let encrypted = codec()
        .encrypt(f.snapshot(b"original key and content"))
        .unwrap();
    let len = encrypted.ciphertext().len();
    let descriptor =
        Descriptor::from_private_event_json(encrypted.descriptor().private_event_json()).unwrap();
    let receipt = store
        .stage(&op("original"), Media::Encrypted(encrypted))
        .map_err(|e| e.error())
        .unwrap();
    let matches = |ns: &[u8; 32], operation: &OperationId, length, descriptor: &Descriptor| {
        encrypted_receipt_matches(ns, operation, length, descriptor, receipt.digest())
    };
    assert!(matches(
        namespace().digest(),
        &op("original"),
        len,
        &descriptor
    ));
    for field in ["k", "iv", "sha256"] {
        let mut json = descriptor.private_event_json().to_vec();
        let needle = format!("\"{field}\":\"");
        let at = std::str::from_utf8(&json).unwrap().find(&needle).unwrap() + needle.len();
        json[at] = if json[at] == b'A' { b'B' } else { b'A' };
        // All dependent descriptor fields remain syntactically valid, including
        // an independently changed ciphertext hash; only the original receipt
        // can establish this descriptor's association.
        let replacement = Descriptor::from_private_event_json(&json).unwrap();
        assert!(
            !matches(namespace().digest(), &op("original"), len, &replacement),
            "{field}"
        );
    }
    let mut encoded = descriptor.private_event_json().to_vec();
    encoded.push(b' ');
    let encoded = Descriptor::from_private_event_json(&encoded).unwrap();
    assert_eq!(encoded.ciphertext_sha256(), descriptor.ciphertext_sha256());
    assert!(!matches(
        namespace().digest(),
        &op("original"),
        len,
        &encoded
    ));
    let other = codec()
        .encrypt(f.snapshot(b"coherent other ciphertext"))
        .unwrap();
    assert!(!matches(
        namespace().digest(),
        &op("original"),
        other.ciphertext().len(),
        other.descriptor()
    ));
    assert!(!matches(
        HostNamespace::new("other").unwrap().digest(),
        &op("original"),
        len,
        &descriptor
    ));
    assert!(!matches(
        namespace().digest(),
        &op("changed"),
        len,
        &descriptor
    ));
    for length in [0, len - 1, len + 1, MAX_ITEM_BYTES + 1, usize::MAX] {
        assert!(!matches(
            namespace().digest(),
            &op("original"),
            length,
            &descriptor
        ));
    }
    let plain = store
        .stage(&op("plain"), f.plain(b"original key and content"))
        .map_err(|e| e.error())
        .unwrap();
    assert!(!encrypted_receipt_matches(
        namespace().digest(),
        &op("plain"),
        len,
        &descriptor,
        plain.digest()
    ));
    assert_eq!(store.committed_records(), 2);
}
