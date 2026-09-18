//! Immutable authenticated replay/source/settled-send proofs, not authority.
//! A compressed binary trie bounds any lookup/update to 256 branches without
//! loading or deleting all historical receipts. The protected journal owns
//! its root; orphaned nodes after failed root publication grant nothing.
use super::{MAX_JOURNAL_BYTES, MAX_SYNCS, Sdk};
use crate::{
    Error,
    event_batch::{
        Receipt,
        disposition::{self, Disposition, Source},
    },
};
use hagency_core::canonical;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    Sync {
        token: String,
        digest: String,
        receipt: Option<Receipt>,
    },
    Source {
        disposition: Disposition,
    },
    Outgoing {
        receipt: crate::outgoing::state::Receipt,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk::{Init, Owner, upload_state};
    use std::sync::Arc;

    async fn sdk() -> (tempfile::TempDir, Init, Sdk) {
        let root = tempfile::tempdir().unwrap();
        let config = crate::sdk::tests::config(root.path());
        let owner = Owner::open(&config).await.unwrap();
        owner.close().await.unwrap();
        let init = Init {
            enrollment: config.enrollment.clone(),
            upload_context: upload_state::Context::new(&config).unwrap(),
            upload_epoch: Arc::new(()),
            approval: false,
            existing: true,
            root: config.root.clone(),
            key: config.key,
            binding: config.binding().unwrap(),
            user: config.identity.transport.sender_mxid.clone(),
            device: config.identity.transport.device_id.clone(),
        };
        let sdk = Sdk::open(&init, false).await.unwrap();
        (root, init, sdk)
    }
    fn response(index: usize) -> Value {
        json!({"next_batch":format!("history_{index}"),"rooms":{"join":{}},"to_device":{"events":[]}})
    }

    #[tokio::test]
    async fn native_matrix_sync_history_rollover() {
        let (_root, init, mut sdk) = sdk().await;
        let identity = sdk.identity.clone();
        for index in 0..150 {
            sdk.sync(response(index)).await.unwrap();
        }
        assert_eq!(sdk.journal.receipts.len(), MAX_SYNCS);
        assert!(sdk.journal.sync_history.is_some());
        sdk.sync(response(0)).await.unwrap();
        assert_eq!(
            sdk.client.sync_token().await.as_deref(),
            Some("history_149")
        );
        assert_eq!(sdk.journal.receipts.len(), MAX_SYNCS);
        sdk.close().await.unwrap();
        drop(sdk);
        let mut sdk = Sdk::open(&init, false).await.unwrap();
        assert_eq!(sdk.identity, identity);
        for index in [0, 1, 30, 85, 100, 149] {
            sdk.sync(response(index)).await.unwrap();
        }
        assert_eq!(
            sdk.client.sync_token().await.as_deref(),
            Some("history_149")
        );
        assert_eq!(sdk.journal.receipts.len(), MAX_SYNCS);
        sdk.close().await.unwrap();
    }

    #[tokio::test]
    async fn native_matrix_sync_history_corruption() {
        for corruption in ["missing", "identity", "bit", "digest", "root_commit"] {
            let (_root, init, mut sdk) = sdk().await;
            for index in 0..MAX_SYNCS {
                sdk.sync(response(index)).await.unwrap();
            }
            let old = sdk.journal.receipts[0].clone();
            if corruption == "root_commit" {
                // An orphan may have reached SQLite before root publication.
                let orphan = sdk
                    .history_insert(
                        None,
                        Record::Sync {
                            token: old.0,
                            digest: old.1,
                            receipt: None,
                        },
                    )
                    .await
                    .unwrap();
                let sql =
                    rusqlite::Connection::open(init.root.join(super::super::DATABASES[0])).unwrap();
                sql.execute_batch("CREATE TRIGGER history_root_abort BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture root commit rollback'); END;").unwrap();
                assert_eq!(sdk.sync(response(64)).await, Err(Error::OutcomeUnknown));
                assert!(sdk.journal.sync_history.is_none());
                assert_eq!(sdk.journal.receipts.len(), MAX_SYNCS);
                assert_eq!(sdk.client.sync_token().await.as_deref(), Some("history_63"));
                sql.execute_batch("DROP TRIGGER history_root_abort")
                    .unwrap();
                assert!(sdk.history_node(&orphan).await.is_ok());
                assert!(
                    !sdk.archived_sync("history_0", &digest(&response(0)).unwrap())
                        .await
                        .unwrap()
                );
                sdk.sync(response(64)).await.unwrap();
            } else {
                sdk.sync(response(64)).await.unwrap();
                let hash = sdk.journal.sync_history.clone().unwrap();
                let mut node = sdk.history_node(&hash).await.unwrap();
                match corruption {
                    "missing" => {
                        sdk.client
                            .state_store()
                            .remove_custom_value(&storage_key(&hash).unwrap())
                            .await
                            .unwrap();
                    }
                    "identity" => {
                        let Node::Leaf { identity, .. } = &mut node else {
                            panic!("first archive is a leaf")
                        };
                        *identity = "foreign-sdk-identity".into();
                        let hash = digest(&node).unwrap();
                        sdk.client
                            .state_store()
                            .set_custom_value(
                                &storage_key(&hash).unwrap(),
                                sdk.cipher.encrypt_value(&node).unwrap(),
                            )
                            .await
                            .unwrap();
                        sdk.journal.sync_history = Some(hash);
                    }
                    "bit" => {
                        node = Node::Branch {
                            identity: sdk.identity.clone(),
                            bit: 256,
                            left: hash.clone(),
                            right: "a".repeat(64),
                        };
                        let hash = digest(&node).unwrap();
                        sdk.client
                            .state_store()
                            .set_custom_value(
                                &storage_key(&hash).unwrap(),
                                sdk.cipher.encrypt_value(&node).unwrap(),
                            )
                            .await
                            .unwrap();
                        sdk.journal.sync_history = Some(hash);
                    }
                    _ => {
                        let Node::Leaf { value, .. } = &mut node else {
                            panic!("first archive is a leaf")
                        };
                        let Record::Sync { token, .. } = value.as_mut() else {
                            panic!("sync proof")
                        };
                        *token = "substituted".into();
                        sdk.client
                            .state_store()
                            .set_custom_value(
                                &storage_key(&hash).unwrap(),
                                sdk.cipher.encrypt_value(&node).unwrap(),
                            )
                            .await
                            .unwrap();
                    }
                }
                assert_eq!(
                    sdk.sync(response(0)).await,
                    Err(Error::Storage),
                    "{corruption}"
                );
                assert_eq!(sdk.client.sync_token().await.as_deref(), Some("history_64"));
            }
            sdk.close().await.unwrap();
        }
    }
}
impl Record {
    fn key(&self) -> Result<String, Error> {
        match self {
            Self::Sync { token, digest, .. } => sync_key(token, digest),
            Self::Source { disposition } => disposition.archive_key(),
            Self::Outgoing { receipt } => {
                crate::outgoing::state::receipt_key(&receipt.id, receipt.fence)
            }
        }
    }
    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Sync {
                token,
                digest,
                receipt,
            } => {
                if token.is_empty() || token.len() > 4096 || !hex(digest) {
                    return Err(Error::Storage);
                }
                if let Some(receipt) = receipt {
                    receipt.validate_dispositions()?;
                    if receipt.token != *token
                        || receipt.digest != *digest
                        || !hex(&receipt.target_digest)
                        || receipt.lacks_filtered_history()
                        || receipt
                            .acknowledgements
                            .len()
                            .checked_add(receipt.filtered)
                            .is_none_or(|n| n > crate::event_batch::MAX_TIMELINE)
                    {
                        return Err(Error::Storage);
                    }
                }
            }
            Self::Source { disposition } => disposition.validate_archive()?,
            Self::Outgoing { receipt } => receipt.validate()?,
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Node {
    Leaf {
        identity: String,
        key: String,
        value: Box<Record>,
    },
    Branch {
        identity: String,
        bit: u16,
        left: String,
        right: String,
    },
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn digest(value: &impl Serialize) -> Result<String, Error> {
    canonical::transport_digest(&serde_json::to_value(value).map_err(|_| Error::Storage)?)
        .map_err(|_| Error::Storage)
}
fn sync_key(token: &str, response: &str) -> Result<String, Error> {
    digest(&json!(["sync_response", token, response]))
}
fn direction(key: &str, bit: u16) -> Result<bool, Error> {
    if !hex(key) || bit >= 256 {
        return Err(Error::Storage);
    }
    let nibble = key.as_bytes()[usize::from(bit / 4)];
    let value = match nibble {
        b'0'..=b'9' => nibble - b'0',
        b'a'..=b'f' => nibble - b'a' + 10,
        _ => return Err(Error::Storage),
    };
    Ok(value & (1 << (3 - bit % 4)) != 0)
}
fn storage_key(hash: &str) -> Result<Vec<u8>, Error> {
    if !hex(hash) {
        return Err(Error::Storage);
    }
    Ok(format!("hagency.sync.history.v1.{hash}").into_bytes())
}
impl Node {
    fn validate(&self, expected: &str) -> Result<(), Error> {
        match self {
            Self::Leaf {
                identity,
                key,
                value,
            } => {
                value.validate()?;
                if identity != expected || !hex(key) || value.key()? != *key {
                    return Err(Error::Storage);
                }
            }
            Self::Branch {
                identity,
                bit,
                left,
                right,
            } => {
                if identity != expected || *bit >= 256 || !hex(left) || !hex(right) || left == right
                {
                    return Err(Error::Storage);
                }
            }
        }
        Ok(())
    }
}
impl Sdk {
    pub(super) async fn settled_outgoing_receipt(
        &self,
        id: &str,
        fence: u64,
    ) -> Result<Option<crate::outgoing::state::Receipt>, Error> {
        let key = crate::outgoing::state::receipt_key(id, fence)?;
        let mut matches = self
            .journal
            .outgoing_receipts
            .iter()
            .filter(|receipt| receipt.id == id && receipt.fence == fence);
        let hot = matches.next().cloned();
        if matches.next().is_some() {
            return Err(Error::Storage);
        }
        if let Some(receipt) = &hot {
            receipt.validate()?;
        }
        let archived = match self
            .history_lookup(self.journal.sync_history.as_deref(), &key)
            .await?
        {
            None => None,
            Some(Record::Outgoing { receipt }) => Some(receipt),
            Some(_) => return Err(Error::Storage),
        };
        match (hot, archived) {
            (Some(hot), Some(archived)) if hot != archived => Err(Error::Storage),
            (Some(receipt), _) | (_, Some(receipt)) => Ok(Some(receipt)),
            (None, None) => Ok(None),
        }
    }
    /// Prepare immutable nodes only. The caller atomically publishes the root
    /// with its cache/attempt change; orphaned nodes do not remove live proof.
    pub(super) async fn prepare_outgoing_rollover(&self) -> Result<Option<String>, Error> {
        if self.journal.outgoing_receipts.len() < crate::outgoing::state::MAX_RECEIPTS {
            return Ok(None);
        }
        if self.journal.outgoing_receipts.len() != crate::outgoing::state::MAX_RECEIPTS {
            return Err(Error::Storage);
        }
        let receipt = self
            .journal
            .outgoing_receipts
            .first()
            .ok_or(Error::Storage)?
            .clone();
        self.history_insert(
            self.journal.sync_history.as_deref(),
            Record::Outgoing { receipt },
        )
        .await
        .map(Some)
    }
    #[cfg(test)]
    pub(super) async fn archive_settled_file_fixture(&mut self) {
        assert!(
            self.journal.outgoing.is_none(),
            "only actual settled source custody can be archived"
        );
        let index = self
            .journal
            .outgoing_receipts
            .iter()
            .position(|receipt| receipt.kind == crate::outgoing::state::Kind::File)
            .expect("actual completed File Settle");
        let receipt = self.journal.outgoing_receipts[index].clone();
        let root = self
            .history_insert(
                self.journal.sync_history.as_deref(),
                Record::Outgoing { receipt },
            )
            .await
            .unwrap();
        self.journal.sync_history = Some(root);
        self.journal.outgoing_receipts.remove(index);
        self.persist().await.unwrap();
    }
    async fn history_node(&self, hash: &str) -> Result<Node, Error> {
        let bytes = self
            .client
            .state_store()
            .get_custom_value(&storage_key(hash)?)
            .await
            .map_err(|_| Error::Storage)?
            .ok_or(Error::Storage)?;
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err(Error::Storage);
        }
        let node: Node = self
            .cipher
            .decrypt_value(&bytes)
            .map_err(|_| Error::Storage)?;
        node.validate(&self.identity)?;
        if digest(&node)? != hash {
            return Err(Error::Storage);
        }
        Ok(node)
    }
    async fn history_write(&self, node: Node) -> Result<String, Error> {
        node.validate(&self.identity)?;
        let hash = digest(&node)?;
        let key = storage_key(&hash)?;
        if self
            .client
            .state_store()
            .get_custom_value(&key)
            .await
            .map_err(|_| Error::Storage)?
            .is_some()
        {
            self.history_node(&hash).await?; // Never overwrite conflicting/corrupt proof.
            return Ok(hash);
        }
        let bytes = self
            .cipher
            .encrypt_value(&node)
            .map_err(|_| Error::Storage)?;
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err(Error::Capacity);
        }
        self.client
            .state_store()
            .set_custom_value(&key, bytes)
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        Ok(hash)
    }
    async fn history_lookup(&self, root: Option<&str>, key: &str) -> Result<Option<Record>, Error> {
        let Some(root) = root else {
            return Ok(None);
        };
        let mut current = root.to_owned();
        let mut path = Vec::new();
        loop {
            match self.history_node(&current).await? {
                Node::Leaf {
                    key: leaf, value, ..
                } => {
                    for (bit, right) in &path {
                        if direction(&leaf, *bit)? != *right {
                            return Err(Error::Storage);
                        }
                    }
                    return Ok((leaf == key).then_some(*value));
                }
                Node::Branch {
                    bit, left, right, ..
                } => {
                    if path.last().is_some_and(|(previous, _)| *previous >= bit)
                        || path.len() >= 256
                    {
                        return Err(Error::Storage);
                    }
                    let goes_right = direction(key, bit)?;
                    path.push((bit, goes_right));
                    current = if goes_right { right } else { left };
                }
            }
        }
    }
    async fn history_insert(&self, root: Option<&str>, value: Record) -> Result<String, Error> {
        value.validate()?;
        let key = value.key()?;
        if let Some(existing) = self.history_lookup(root, &key).await? {
            // The earliest terminal source is immutable, including after trust
            // or presentation changes. Exact sync receipts cannot be rewritten.
            if matches!(
                (&existing, &value),
                (Record::Source { .. }, Record::Source { .. })
            ) || digest(&existing)? == digest(&value)?
            {
                return Ok(root.ok_or(Error::Storage)?.to_owned());
            }
            return Err(Error::Conflict);
        }
        let leaf = self
            .history_write(Node::Leaf {
                identity: self.identity.clone(),
                key: key.clone(),
                value: Box::new(value),
            })
            .await?;
        let Some(root) = root else {
            return Ok(leaf);
        };
        // Find the differing bit, then insert above the first deeper branch.
        let mut current = root.to_owned();
        let mut previous = None;
        let other = loop {
            match self.history_node(&current).await? {
                Node::Leaf { key, .. } => break key,
                Node::Branch {
                    bit, left, right, ..
                } => {
                    if previous.is_some_and(|value| value >= bit) {
                        return Err(Error::Storage);
                    }
                    previous = Some(bit);
                    current = if direction(&key, bit)? { right } else { left };
                }
            }
        };
        let mut differing = None;
        for bit in 0..256 {
            if direction(&key, bit)? != direction(&other, bit)? {
                differing = Some(bit);
                break;
            }
        }
        let bit = differing.ok_or(Error::Storage)?;
        current = root.to_owned();
        let mut ancestors = Vec::new();
        let mut previous = None;
        loop {
            let node = self.history_node(&current).await?;
            match &node {
                Node::Branch {
                    bit: existing,
                    left,
                    right,
                    ..
                } if *existing < bit => {
                    if previous.is_some_and(|value| value >= *existing) || ancestors.len() >= 256 {
                        return Err(Error::Storage);
                    }
                    previous = Some(*existing);
                    let goes_right = direction(&key, *existing)?;
                    current = if goes_right {
                        right.clone()
                    } else {
                        left.clone()
                    };
                    ancestors.push((node, goes_right));
                }
                _ => break,
            }
        }
        let (left, right) = if direction(&key, bit)? {
            (current, leaf)
        } else {
            (leaf, current)
        };
        let mut updated = self
            .history_write(Node::Branch {
                identity: self.identity.clone(),
                bit,
                left,
                right,
            })
            .await?;
        for (node, goes_right) in ancestors.into_iter().rev() {
            let Node::Branch {
                identity,
                bit,
                mut left,
                mut right,
            } = node
            else {
                return Err(Error::Storage);
            };
            if goes_right {
                right = updated;
            } else {
                left = updated;
            }
            updated = self
                .history_write(Node::Branch {
                    identity,
                    bit,
                    left,
                    right,
                })
                .await?;
        }
        Ok(updated)
    }
    pub(super) async fn archived_sync(&self, token: &str, response: &str) -> Result<bool, Error> {
        match self
            .history_lookup(
                self.journal.sync_history.as_deref(),
                &sync_key(token, response)?,
            )
            .await?
        {
            Some(Record::Sync {
                token: observed,
                digest,
                ..
            }) if observed == token && digest == response => Ok(true),
            None => Ok(false),
            _ => Err(Error::Storage),
        }
    }
    pub(super) async fn archived_sources(&self, raw: &Value) -> Result<Vec<Disposition>, Error> {
        let mut history = Vec::new();
        for (room, event) in disposition::raw_events(raw)? {
            let key = Source::new(&room, &event)?.archive_key()?;
            match self
                .history_lookup(self.journal.sync_history.as_deref(), &key)
                .await?
            {
                Some(Record::Source { disposition }) => history.push(disposition),
                None => {}
                _ => return Err(Error::Storage),
            }
        }
        Ok(history) // At most MAX_TIMELINE records, regardless of total history.
    }
    pub(super) async fn sync_receipt_room(&mut self) -> Result<(), Error> {
        if self.journal.receipts.len() < MAX_SYNCS {
            return Ok(());
        }
        if self.journal.receipts.len() != MAX_SYNCS
            || self.journal.pending.is_some()
            || self.journal.intake.is_some()
        {
            return Err(Error::OutcomeUnknown);
        }
        let (token, response) = self
            .journal
            .receipts
            .first()
            .cloned()
            .ok_or(Error::Storage)?;
        let index = self
            .journal
            .intake_receipts
            .iter()
            .position(|receipt| receipt.token == token && receipt.digest == response);
        let receipt = index.map(|index| self.journal.intake_receipts[index].clone());
        let mut root = self.journal.sync_history.clone();
        if let Some(receipt) = &receipt {
            for disposition in receipt
                .dispositions
                .iter()
                .flatten()
                .filter(|value| value.terminal())
            {
                root = Some(
                    self.history_insert(
                        root.as_deref(),
                        Record::Source {
                            disposition: disposition.clone(),
                        },
                    )
                    .await?,
                );
            }
        }
        root = Some(
            self.history_insert(
                root.as_deref(),
                Record::Sync {
                    token,
                    digest: response,
                    receipt,
                },
            )
            .await?,
        );
        let previous = self
            .journal
            .sync_history
            .replace(root.ok_or(Error::Storage)?);
        let old = self.journal.receipts.remove(0);
        let old_intake = index.map(|index| (index, self.journal.intake_receipts.remove(index)));
        if self.persist().await.is_err() {
            self.journal.sync_history = previous;
            self.journal.receipts.insert(0, old);
            if let Some((index, receipt)) = old_intake {
                self.journal.intake_receipts.insert(index, receipt);
            }
            return Err(Error::OutcomeUnknown);
        }
        Ok(())
    }
}
