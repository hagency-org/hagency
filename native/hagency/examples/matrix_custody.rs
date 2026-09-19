//! Operator-only read-only encrypted journal inspection. No SDK open, network,
//! key reset, mutation, or private message/identity projection.
use hagency_store::private;
use matrix_sdk_store_encryption::StoreCipher;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

fn hash(value: &Value) -> Result<String, String> {
    hagency_core::canonical::transport_digest(value).map_err(|_| "history digest refused".into())
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn fields(value: &Value, names: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == names.len() && names.iter().all(|name| object.contains_key(*name))
    })
}
fn receipt_key(id: &str, fence: u64) -> Result<String, String> {
    if id.is_empty()
        || id.len() > 128
        || id.chars().any(char::is_control)
        || fence == 0
        || fence > hagency_core::JSON_SAFE_MAX
    {
        return Err("requested receipt identity refused".into());
    }
    hash(&json!(["settled_outgoing_receipt", id, fence]))
}
fn settled_key(receipt: &Value) -> Result<String, String> {
    if !fields(receipt, &["kind", "id", "fence", "attempt_digest"])
        || !receipt["kind"]
            .as_str()
            .is_some_and(|kind| ["Final", "Notice", "File"].contains(&kind))
        || !receipt["attempt_digest"].as_str().is_some_and(hex)
    {
        return Err("settled receipt shape refused".into());
    }
    receipt_key(
        receipt["id"]
            .as_str()
            .ok_or("settled receipt shape refused")?,
        receipt["fence"]
            .as_u64()
            .ok_or("settled receipt shape refused")?,
    )
}
fn bit(key: &str, index: u64) -> Result<bool, String> {
    if !hex(key) || index >= 256 {
        return Err("history branch refused".into());
    }
    let byte = key.as_bytes()[index as usize / 4];
    let nibble = if byte <= b'9' {
        byte - b'0'
    } else {
        byte - b'a' + 10
    };
    Ok(nibble & (1 << (3 - index % 4)) != 0)
}
fn record_key(record: &Value) -> Result<String, String> {
    // This diagnostic checks factual key linkage, not full SDK intake authority.
    match record["kind"].as_str() {
        Some("outgoing") if fields(record, &["kind", "receipt"]) => settled_key(&record["receipt"]),
        Some("sync")
            if fields(record, &["kind", "token", "digest", "receipt"])
                && record["token"]
                    .as_str()
                    .is_some_and(|token| !token.is_empty() && token.len() <= 4096)
                && record["digest"].as_str().is_some_and(hex) =>
        {
            hash(&json!(["sync_response", record["token"], record["digest"]]))
        }
        Some("source") if fields(record, &["kind", "disposition"]) => {
            let disposition = &record["disposition"];
            let source = &disposition["source"];
            let decision = &disposition["decision"];
            let terminal = match decision["kind"].as_str() {
                Some("not_target") => fields(decision, &["kind"]),
                Some("rejected") => {
                    fields(decision, &["kind", "reason"])
                        && decision["reason"].as_str().is_some_and(|reason| {
                            [
                                "stale_session",
                                "malformed",
                                "unsupported",
                                "crypto_ineligible",
                                "plaintext_encrypted",
                                "source_conflict",
                            ]
                            .contains(&reason)
                        })
                }
                _ => false,
            };
            if !fields(disposition, &["source", "sdk_observation", "decision"])
                || !fields(source, &["room", "event", "raw", "immutable"])
                || !["room", "raw", "immutable"]
                    .iter()
                    .all(|name| source[*name].as_str().is_some_and(hex))
                || !(source["event"].is_null() || source["event"].as_str().is_some_and(hex))
                || !disposition["sdk_observation"].as_str().is_some_and(hex)
                || !terminal
            {
                return Err("terminal history shape refused".into());
            }
            let event = if source["event"].is_null() {
                &source["raw"]
            } else {
                &source["event"]
            };
            hash(&json!(["terminal_source", source["room"], event]))
        }
        _ => Err("history record shape refused".into()),
    }
}
fn blob(db: &Connection, key: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let bytes: Option<Vec<u8>> = db
        .query_row(
            "SELECT CASE WHEN length(value)<=?2 THEN value ELSE NULL END FROM kv_blob WHERE key=?1",
            rusqlite::params![key, limit],
            |row| row.get(0),
        )
        .map_err(|_| "protected record unavailable")?;
    bytes.ok_or_else(|| "protected record exceeds bound".into())
}
fn archived_receipt(
    db: &Connection,
    sdk: &StoreCipher,
    cipher: &StoreCipher,
    identity: &str,
    root: &Value,
    key: &str,
) -> Result<(Option<Value>, usize), String> {
    if root.is_null() {
        return Ok((None, 0));
    }
    let mut current = root
        .as_str()
        .filter(|root| hex(root))
        .ok_or("history root refused")?
        .to_owned();
    let mut path: Vec<(u64, bool)> = Vec::new();
    loop {
        let encoded = sdk.hash_key(
            "kv_blob",
            format!("custom:hagency.sync.history.v1.{current}").as_bytes(),
        );
        let node: Value = cipher
            .decrypt_value(&blob(db, &encoded, 16 * 1024 * 1024)?)
            .map_err(|_| "history authentication refused")?;
        if node["identity"].as_str() != Some(identity) || hash(&node)? != current {
            return Err("history identity or digest refused".into());
        }
        match node["kind"].as_str() {
            Some("leaf") if fields(&node, &["kind", "identity", "key", "value"]) => {
                let leaf = node["key"]
                    .as_str()
                    .filter(|key| hex(key))
                    .ok_or("history leaf refused")?;
                if record_key(&node["value"])? != leaf
                    || path
                        .iter()
                        .any(|(index, right)| bit(leaf, *index).ok() != Some(*right))
                {
                    return Err("history leaf key or path refused".into());
                }
                let receipt = if leaf == key {
                    if node["value"]["kind"] != "outgoing" {
                        return Err("settled history kind refused".into());
                    }
                    Some(node["value"]["receipt"].clone())
                } else {
                    None
                };
                return Ok((receipt, path.len() + 1));
            }
            Some("branch") if fields(&node, &["kind", "identity", "bit", "left", "right"]) => {
                let index = node["bit"]
                    .as_u64()
                    .filter(|index| *index < 256)
                    .ok_or("history branch refused")?;
                if path.len() >= 256
                    || path.last().is_some_and(|(previous, _)| *previous >= index)
                    || !["left", "right"]
                        .iter()
                        .all(|name| node[*name].as_str().is_some_and(hex))
                    || node["left"] == node["right"]
                {
                    return Err("history branch refused".into());
                }
                let right = bit(key, index)?;
                path.push((index, right));
                current = node[if right { "right" } else { "left" }]
                    .as_str()
                    .ok_or("history branch refused")?
                    .to_owned();
            }
            _ => return Err("history node shape refused".into()),
        }
    }
}

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = private::open(path, false).map_err(|_| "private file refused")?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "private read refused")?;
    if bytes.len() > limit {
        return Err("private file exceeds bound".into());
    }
    Ok(bytes)
}
fn inspect(state: &Path) -> Result<Value, String> {
    inspect_for(state, None)
}
fn inspect_for(state: &Path, requested: Option<(&str, u64)>) -> Result<Value, String> {
    let requested = requested
        .map(|(id, fence)| receipt_key(id, fence))
        .transpose()?;
    if !state.is_absolute() {
        return Err("absolute existing state dir required".into());
    }
    let root = state.join("sdk");
    let key: [u8; 32] = read(&state.join("matrix.sdk_key"), 32)?
        .try_into()
        .map_err(|_| "key length refused")?;
    private::open(&root.join("matrix-sdk-state.sqlite3"), false)
        .map_err(|_| "private database refused")?;
    let db = Connection::open_with_flags(
        root.join("matrix-sdk-state.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|_| "read-only database refused")?;
    let cipher_bytes: Vec<u8> = db
        .query_row("SELECT value FROM kv WHERE key='cipher'", [], |row| {
            row.get(0)
        })
        .map_err(|_| "SDK cipher record unavailable")?;
    if cipher_bytes.len() > 1024 {
        return Err("SDK cipher record exceeds bound".into());
    }
    let sdk =
        StoreCipher::import_with_key(&key, &cipher_bytes).map_err(|_| "SDK cipher refused")?;
    let encoded = sdk.hash_key("kv_blob", b"custom:hagency.observer.sync.v1");
    let bytes = blob(&db, &encoded, 16 * 1024 * 1024)?;
    let cipher = StoreCipher::import_with_key(&key, &read(&root.join("journal.key"), 1024)?)
        .map_err(|_| "journal cipher refused")?;
    let journal: Value = cipher
        .decrypt_value(&bytes)
        .map_err(|_| "journal authentication refused")?;
    let hot = match journal.get("outgoing_receipts") {
        None => &[][..],
        Some(Value::Array(values)) if values.len() <= 64 => values.as_slice(),
        _ => return Err("hot outgoing receipts exceed bound or shape refused".into()),
    };
    let receipt = if let Some(key) = requested {
        let identity = String::from_utf8(read(&root.join("identity"), 256)?)
            .map_err(|_| "SDK identity refused")?;
        if identity.is_empty() {
            return Err("SDK identity refused".into());
        }
        let mut matching = None;
        for value in hot {
            if settled_key(value)? == key && matching.replace(value).is_some() {
                return Err("duplicate hot receipt refused".into());
            }
        }
        let (archived, visited) = archived_receipt(
            &db,
            &sdk,
            &cipher,
            &identity,
            &journal["sync_history"],
            &key,
        )?;
        if matching
            .zip(archived.as_ref())
            .is_some_and(|(hot, archived)| hot != archived)
        {
            return Err("hot and archived receipt conflict".into());
        }
        Some(
            json!({"found_in_hot":matching.is_some(),"found_in_archive":archived.is_some(),
            "kind":matching.or(archived.as_ref()).map(|receipt| &receipt["kind"]),"visited_nodes":visited}),
        )
    } else {
        None
    };
    let batch = journal.get("intake").filter(|value| !value.is_null());
    let reasons = [
        "historical receipt conflicts with frozen scope or content",
        "unadmitted event has a retired source incarnation",
        "domain refused the frozen event scope or content",
        "historical source proof or incomplete timeline refused",
        "unsupported SDK event or incomplete timeline",
        "attachment manifest capacity or identity refused",
    ];
    Ok(json!({"read_only":true,
    "hot_receipts":journal["receipts"].as_array().map(Vec::len),
    "intake_receipts":journal["intake_receipts"].as_array().map(Vec::len),
    "outgoing_hot_receipts":hot.len(),
    "pending_outgoing":journal.get("outgoing").is_some_and(|value| !value.is_null()),
    "outgoing_phase":journal["outgoing"]["phase"].as_str().filter(|phase| ["BeforeBegin", "Ready", "QueryPrepared",
        "CryptoApplying", "WritePossible", "ResponseStored", "Complete", "Quarantined"].contains(phase)),
    "receipt":receipt,
    "archived":journal.get("sync_history").is_some_and(|value| !value.is_null()),
    "pending_observation":journal.get("pending").is_some_and(|value| !value.is_null()),
    "batch":batch.map(|batch| json!({
        "phase":batch["phase"].as_str().filter(|phase| ["prepared","applying","derived","quarantined"].contains(phase)),
        "digest":batch["digest"].as_str().filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())),
        "reason":batch["reason"].as_str().filter(|reason| reasons.contains(reason)).map(|reason| reasons.iter().position(|candidate| candidate == &reason).unwrap()),
        "events":batch["events"].as_array().map(Vec::len),
        "acknowledged":batch["acknowledgements"].as_array().map(Vec::len),
        "targets":batch["targets"].as_array().map(Vec::len),
    }))}))
}
fn main() {
    #[derive(clap::Parser)]
    struct Arguments {
        state: PathBuf,
        #[arg(long, requires = "fence")]
        receipt: Option<String>,
        #[arg(long, requires = "receipt")]
        fence: Option<u64>,
    }
    let arguments = <Arguments as clap::Parser>::parse();
    let result = match (arguments.receipt.as_deref(), arguments.fence) {
        (Some(id), Some(fence)) => inspect_for(&arguments.state, Some((id, fence))),
        (None, None) => inspect(&arguments.state),
        _ => Err("requested receipt identity refused".into()),
    };
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("matrix_custody: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(journal: &Value) -> tempfile::TempDir {
        let temporary = tempfile::tempdir().unwrap();
        let state = temporary.path().join("state");
        private::create_directory_new(&state).unwrap();
        let root = state.join("sdk");
        private::directory(&root).unwrap();
        let key = [42; 32];
        private::write_new(&state.join("matrix.sdk_key"), &key).unwrap();
        let sdk = StoreCipher::new().unwrap();
        let cipher = StoreCipher::new().unwrap();
        private::write_new(
            &root.join("journal.key"),
            &cipher.export_with_key(&key).unwrap(),
        )
        .unwrap();
        private::write_new(
            &root.join("identity"),
            b"PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED",
        )
        .unwrap();
        let path = root.join("matrix-sdk-state.sqlite3");
        private::write_new(&path, &[]).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("CREATE TABLE kv(key TEXT PRIMARY KEY, value BLOB); CREATE TABLE kv_blob(key BLOB PRIMARY KEY, value BLOB);").unwrap();
        db.execute(
            "INSERT INTO kv VALUES('cipher',?1)",
            [sdk.export_with_key(&key).unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO kv_blob VALUES(?1,?2)",
            rusqlite::params![
                sdk.hash_key("kv_blob", b"custom:hagency.observer.sync.v1")
                    .as_slice(),
                cipher.encrypt_value(journal).unwrap()
            ],
        )
        .unwrap();
        temporary
    }

    fn settled(id: &str) -> Value {
        json!({"kind":"Final","id":id,"fence":1,"attempt_digest":"e".repeat(64)})
    }
    fn leaf(receipt: Value) -> Value {
        json!({"kind":"leaf","identity":"PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED",
            "key":settled_key(&receipt).unwrap(),"value":{"kind":"outgoing","receipt":receipt}})
    }
    fn store_node(state: &Path, address: &str, node: &Value) {
        let db = Connection::open(state.join("sdk/matrix-sdk-state.sqlite3")).unwrap();
        let exported: Vec<u8> = db
            .query_row("SELECT value FROM kv WHERE key='cipher'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let sdk = StoreCipher::import_with_key(&[42; 32], &exported).unwrap();
        let cipher = StoreCipher::import_with_key(
            &[42; 32],
            &read(&state.join("sdk/journal.key"), 1024).unwrap(),
        )
        .unwrap();
        let key = sdk.hash_key(
            "kv_blob",
            format!("custom:hagency.sync.history.v1.{address}").as_bytes(),
        );
        db.execute(
            "INSERT OR REPLACE INTO kv_blob VALUES(?1,?2)",
            rusqlite::params![key.as_slice(), cipher.encrypt_value(node).unwrap()],
        )
        .unwrap();
    }

    #[test]
    fn native_matrix_custody_settled_receipt() {
        let old = settled("PRIVATE_OLD_RECEIPT_ID");
        let newer = settled("PRIVATE_NEW_RECEIPT_ID");
        let a = leaf(old.clone());
        let b = leaf(newer.clone());
        let a_hash = hash(&a).unwrap();
        let b_hash = hash(&b).unwrap();
        let index = (0..256)
            .find(|index| {
                bit(a["key"].as_str().unwrap(), *index).unwrap()
                    != bit(b["key"].as_str().unwrap(), *index).unwrap()
            })
            .unwrap();
        let (left, right) = if bit(a["key"].as_str().unwrap(), index).unwrap() {
            (&b_hash, &a_hash)
        } else {
            (&a_hash, &b_hash)
        };
        let root = json!({"kind":"branch","identity":"PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED","bit":index,"left":left,"right":right});
        let root_hash = hash(&root).unwrap();
        let temporary =
            fixture(&json!({"outgoing_receipts":[newer],"sync_history":root_hash,"outgoing":null}));
        let state = temporary.path().join("state");
        store_node(&state, &a_hash, &a);
        store_node(&state, &b_hash, &b);
        store_node(&state, &root_hash, &root);
        let database = state.join("sdk/matrix-sdk-state.sqlite3");
        let before = std::fs::read(&database).unwrap();
        let result = inspect_for(&state, Some(("PRIVATE_OLD_RECEIPT_ID", 1))).unwrap();
        assert_eq!(
            result["receipt"],
            json!({"found_in_hot":false,"found_in_archive":true,"kind":"Final","visited_nodes":2})
        );
        assert!(!result.to_string().contains("PRIVATE"));
        assert_eq!(
            inspect_for(&state, Some(("PRIVATE_NEW_RECEIPT_ID", 1))).unwrap()["receipt"]["found_in_hot"],
            true
        );
        assert_eq!(
            inspect_for(&state, Some(("PRIVATE_NEW_RECEIPT_ID", 1))).unwrap()["receipt"]["found_in_archive"],
            true
        );
        assert_eq!(
            inspect_for(&state, Some(("PRIVATE_OLD_RECEIPT_ID", 2))).unwrap()["receipt"]["found_in_archive"],
            false
        );
        assert_eq!(std::fs::read(&database).unwrap(), before);
        assert_eq!(
            inspect_for(&state, Some(("PRIVATE_OLD_RECEIPT_ID", 1))).unwrap(),
            result
        );
    }

    #[test]
    fn native_matrix_custody_settled_receipt_refusals() {
        for variant in [
            "missing",
            "digest",
            "identity",
            "conflict",
            "duplicate",
            "branch",
            "path",
            "root",
        ] {
            let receipt = settled("PRIVATE_RECEIPT_ID");
            let mut node = leaf(receipt.clone());
            if variant == "identity" {
                node["identity"] = json!("FOREIGN_PRIVATE_IDENTITY");
            }
            if variant == "branch" {
                node = json!({"kind":"branch","identity":"PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED","bit":256,
                    "left":"a".repeat(64),"right":"b".repeat(64)});
            }
            let address = hash(&node).unwrap();
            let mut hot = receipt.clone();
            if variant == "conflict" {
                hot["attempt_digest"] = json!("d".repeat(64));
            }
            let hot = if variant == "duplicate" {
                json!([hot.clone(), hot])
            } else {
                json!([hot])
            };
            let temporary = fixture(
                &json!({"outgoing_receipts":hot,"sync_history":if variant == "root" { json!("PRIVATE_ROOT") } else { json!(address) }}),
            );
            let state = temporary.path().join("state");
            if variant == "digest" {
                node["value"]["receipt"]["attempt_digest"] = json!("d".repeat(64));
            }
            if variant != "missing" {
                store_node(&state, &address, &node);
            }
            if variant == "path" {
                // A hashed/authenticated wrong-side leaf is not valid absence.
                let root = json!({"kind":"branch","identity":"PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED","bit":0,
                    "left":if bit(node["key"].as_str().unwrap(),0).unwrap() { json!(address) } else { json!("a".repeat(64)) },
                    "right":if bit(node["key"].as_str().unwrap(),0).unwrap() { json!("a".repeat(64)) } else { json!(address) }});
                // Directly test a valid authenticated root leading to a leaf
                // whose own key contradicts the traversed branch direction.
                let opposite_key = if bit(node["key"].as_str().unwrap(), 0).unwrap() {
                    "0".repeat(64)
                } else {
                    "f".repeat(64)
                };
                let root_hash = hash(&root).unwrap();
                store_node(&state, &root_hash, &root);
                let db = Connection::open_with_flags(
                    state.join("sdk/matrix-sdk-state.sqlite3"),
                    OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .unwrap();
                let exported: Vec<u8> = db
                    .query_row("SELECT value FROM kv WHERE key='cipher'", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                let sdk = StoreCipher::import_with_key(&[42; 32], &exported).unwrap();
                let cipher = StoreCipher::import_with_key(
                    &[42; 32],
                    &read(&state.join("sdk/journal.key"), 1024).unwrap(),
                )
                .unwrap();
                assert!(
                    archived_receipt(
                        &db,
                        &sdk,
                        &cipher,
                        "PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED",
                        &json!(root_hash),
                        &opposite_key
                    )
                    .is_err()
                );
                continue;
            }
            let before = std::fs::read(state.join("sdk/matrix-sdk-state.sqlite3")).unwrap();
            let error = inspect_for(&state, Some(("PRIVATE_RECEIPT_ID", 1))).unwrap_err();
            assert!(!error.contains("PRIVATE"), "{variant}");
            assert_eq!(
                std::fs::read(state.join("sdk/matrix-sdk-state.sqlite3")).unwrap(),
                before
            );
        }
        for (id, fence) in [
            ("", 1),
            ("valid", 0),
            ("bad\n", 1),
            ("valid", hagency_core::JSON_SAFE_MAX + 1),
        ] {
            assert_eq!(
                inspect_for(Path::new("unavailable"), Some((id, fence))).unwrap_err(),
                "requested receipt identity refused"
            );
        }
        assert!(receipt_key(&"x".repeat(129), 1).is_err());
        // Exercise the full allowed path through 256 actual authenticated
        // branch records. The leaf still has to match every traversed bit.
        let node = leaf(settled("PRIVATE_RECEIPT_ID"));
        let key = node["key"].as_str().unwrap().to_owned();
        let leaf_hash = hash(&node).unwrap();
        let temporary = fixture(&json!({"outgoing_receipts":[],"sync_history":null}));
        let state = temporary.path().join("state");
        store_node(&state, &leaf_hash, &node);
        let mut current = leaf_hash;
        for index in (0..256).rev() {
            let right = bit(&key, index).unwrap();
            let node = json!({"kind":"branch","identity":"PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED","bit":index,
                "left":if right { json!("a".repeat(64)) } else { json!(current) },
                "right":if right { json!(current) } else { json!("a".repeat(64)) }});
            current = hash(&node).unwrap();
            store_node(&state, &current, &node);
        }
        let db = Connection::open_with_flags(
            state.join("sdk/matrix-sdk-state.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let exported: Vec<u8> = db
            .query_row("SELECT value FROM kv WHERE key='cipher'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let sdk = StoreCipher::import_with_key(&[42; 32], &exported).unwrap();
        let cipher = StoreCipher::import_with_key(
            &[42; 32],
            &read(&state.join("sdk/journal.key"), 1024).unwrap(),
        )
        .unwrap();
        assert_eq!(
            archived_receipt(
                &db,
                &sdk,
                &cipher,
                "PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED",
                &json!(current),
                &key
            )
            .unwrap()
            .1,
            257
        );
        let repeated = json!({"kind":"branch","identity":"PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED","bit":0,
            "left":if bit(&key,0).unwrap() { json!("a".repeat(64)) } else { json!(current) },
            "right":if bit(&key,0).unwrap() { json!(current) } else { json!("a".repeat(64)) }});
        let repeated_hash = hash(&repeated).unwrap();
        store_node(&state, &repeated_hash, &repeated);
        assert!(
            archived_receipt(
                &db,
                &sdk,
                &cipher,
                "PRIVATE_SDK_IDENTITY_MUST_NOT_BE_PROJECTED",
                &json!(repeated_hash),
                &key
            )
            .is_err()
        );
    }

    #[test]
    fn native_matrix_custody_outgoing_metadata() {
        let receipts: Vec<Value> = (0..64)
            .map(|index| settled(&format!("private_id_{index}")))
            .collect();
        let temporary = fixture(
            &json!({"outgoing_receipts":receipts,"outgoing":{"phase":"WritePossible","content":"PRIVATE_BODY"}}),
        );
        let state = temporary.path().join("state");
        let result = inspect(&state).unwrap();
        assert_eq!(result["outgoing_hot_receipts"], 64);
        assert_eq!(result["pending_outgoing"], true);
        assert_eq!(result["outgoing_phase"], "WritePossible");
        assert!(result["receipt"].is_null());
        assert!(!result.to_string().contains("PRIVATE"));
        assert!(!result.to_string().contains("private_id"));
        let temporary = fixture(&json!({"outgoing_receipts":vec![settled("private");65]}));
        assert!(inspect(&temporary.path().join("state")).is_err());
        let temporary =
            fixture(&json!({"outgoing_receipts":{},"outgoing":{"phase":"PRIVATE_PHASE"}}));
        assert!(inspect(&temporary.path().join("state")).is_err());
        let temporary = fixture(&json!({"outgoing":{"phase":"PRIVATE_PHASE"}}));
        assert!(inspect(&temporary.path().join("state")).unwrap()["outgoing_phase"].is_null());
    }

    #[test]
    fn native_matrix_custody_read_only_and_redacted() {
        let marker = "PRIVATE_BODY_MUST_NOT_BE_PROJECTED";
        let journal = json!({"receipts":[{}],"intake_receipts":[{},{}],"sync_history":{"opaque":marker},
            "pending":null,"intake":{"phase":"quarantined","digest":"f".repeat(64),
                "reason":"domain refused the frozen event scope or content","events":[{"body":marker}],
                "acknowledgements":[],"targets":[{"identity":marker}]},"token":marker});
        let temporary = fixture(&journal);
        let state = temporary.path().join("state");
        let path = state.join("sdk/matrix-sdk-state.sqlite3");
        let before = std::fs::read(&path).unwrap();
        let value = inspect(&state).unwrap();
        assert_eq!(value["batch"]["reason"], 2);
        assert_eq!(value["batch"]["events"], 1);
        assert_eq!(value["hot_receipts"], 1);
        assert_eq!(value["intake_receipts"], 2);
        assert_eq!(value["archived"], true);
        assert!(!value.to_string().contains(marker));
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert!(inspect(Path::new("relative")).is_err());
        assert_eq!(inspect(&state).unwrap(), value);
    }

    #[test]
    fn native_matrix_custody_refuses_key_bounds_and_authentication() {
        let temporary = fixture(
            &json!({"intake":{"phase":"PRIVATE_PHASE","reason":"PRIVATE_REASON","digest":"PRIVATE_DIGEST"}}),
        );
        let state = temporary.path().join("state");
        let value = inspect(&state).unwrap();
        assert!(value["batch"]["phase"].is_null());
        assert!(value["batch"]["reason"].is_null());
        assert!(value["batch"]["digest"].is_null());
        let key = state.join("matrix.sdk_key");
        std::fs::write(&key, [42; 33]).unwrap();
        assert_eq!(inspect(&state).unwrap_err(), "private file exceeds bound");
        std::fs::write(&key, [41; 32]).unwrap();
        assert_eq!(inspect(&state).unwrap_err(), "SDK cipher refused");
    }
}
