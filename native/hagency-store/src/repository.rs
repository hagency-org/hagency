use crate::Error;
use hagency_core::{
    JSON_SAFE_MAX,
    custody::{CustodyState, Delivery, Receipt},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{fs::File, path::Path};

const APPLICATION_ID: i32 = 0x48414731; // HAG1; never accept a JS router or crypto database.
const VERSION: i32 = 2;

pub struct Repository {
    pub(crate) db: Connection,
    pub(crate) max_records: i64,
    pub(crate) max_payload_bytes: i64,
    pub(crate) max_attempts: i64,
    _ownership: File, // Must outlive the connection, including its final checkpoint.
}

impl Repository {
    pub fn open(directory: &Path) -> Result<Self, Error> {
        let database = crate::database::open(
            directory,
            crate::database::Schema {
                name: "custody.sqlite3",
                lock: "owner.lock",
                application_id: APPLICATION_ID,
                version: VERSION,
                migrations: &[(2, include_str!("custody-migrations/002-outbound.sql"))],
                sql: include_str!("schema.sql"),
                verify: &[
                    "SELECT id,lane,binding,generation,digest,payload,receipt FROM inbox LIMIT 0",
                    "SELECT kind,origin_machine_generation,lease_generation,lease_token,lease_expires,lease_state,processing_state,retry_at FROM inbox LIMIT 0",
                    "SELECT binding,identity,fleet_key,consumer,machine_generation,fingerprint,scope,sequence,accepted_sequence,accepted_digest FROM outbound_transports LIMIT 0",
                    "SELECT binding,lane,ticket,response_digest FROM outbound_polls LIMIT 0",
                    "SELECT binding,id,lane,delivery_id,command_digest,capability,state,deadline,result,result_digest FROM outbound_attempts LIMIT 0",
                    "SELECT binding,sequence,digest,body,state FROM outbound_publications LIMIT 0",
                ],
            },
        )?;
        let mut repository = Self {
            db: database.connection,
            max_records: 1024,
            max_payload_bytes: 16 * 1024 * 1024,
            max_attempts: 4096,
            _ownership: database.ownership,
        };
        repository.recover_outbound()?;
        Ok(repository)
    }

    pub fn receive(&mut self, delivery: &Delivery, now_ms: u64) -> Result<Receipt, Error> {
        delivery.validate()?;
        if now_ms > JSON_SAFE_MAX {
            return Err(hagency_core::InvalidInput("invalid UTC timestamp").into());
        }
        let digest = delivery.content_digest()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM outbound_transports WHERE binding=?1)",
            [&delivery.binding],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::Generation);
        }
        let generation: Option<i64> = tx
            .query_row(
                "SELECT generation FROM bindings WHERE id=?1",
                [&delivery.binding],
                |r| r.get(0),
            )
            .optional()?;
        if generation.is_some_and(|g| g != delivery.generation as i64) {
            return Err(Error::Generation);
        }
        let previous: Option<(String, String)> = tx
            .query_row(
                "SELECT digest,receipt FROM inbox WHERE binding=?1 AND lane=?2 AND id=?3",
                params![delivery.binding, delivery.lane.as_str(), delivery.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((previous_digest, receipt)) = previous {
            if digest != previous_digest {
                return Err(Error::Conflict);
            }
            return Ok(serde_json::from_str(&receipt)?);
        }
        let payload = serde_json::to_string(&delivery.payload)?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM inbox", [], |r| r.get(0))?;
        let bytes = crate::outbound::repository::retained_bytes(&tx)?;
        if count >= self.max_records || bytes + payload.len() as i64 > self.max_payload_bytes {
            return Err(Error::Capacity);
        }
        let receipt = Receipt {
            id: delivery.id.clone(),
            lane: delivery.lane,
            generation: delivery.generation,
            digest,
            received_at_ms: now_ms,
            state: CustodyState::Received,
        };
        tx.execute(
            "INSERT INTO bindings(id,generation) VALUES(?1,?2) ON CONFLICT(id) DO NOTHING",
            params![delivery.binding, delivery.generation as i64],
        )?;
        tx.execute("INSERT INTO inbox(binding,lane,id,generation,digest,payload,receipt) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![delivery.binding, delivery.lane.as_str(), delivery.id, delivery.generation as i64, receipt.digest,
                payload, serde_json::to_string(&receipt)?])?;
        tx.commit()?; // Custody cannot be acknowledged before this boundary.
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hagency_core::custody::{Kind, Lane};
    use serde_json::json;
    pub(super) fn delivery() -> Delivery {
        Delivery {
            binding: "fixture_registration".into(),
            generation: 1,
            id: "request_1".into(),
            lane: Lane::Work,
            kind: Kind::Request,
            payload: json!({"name":"小白", "requestedTokens": 100000}),
        }
    }
    #[test]
    fn custody_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let receipt = Repository::open(directory.path().join("state").as_path())
            .unwrap()
            .receive(&delivery(), 1000)
            .unwrap();
        let mut reopened = Repository::open(directory.path().join("state").as_path()).unwrap();
        assert_eq!(reopened.receive(&delivery(), 2000).unwrap(), receipt);
        assert_eq!(receipt.state, CustodyState::Received);
        let payload: String = reopened
            .db
            .query_row("SELECT payload FROM inbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
            delivery().payload
        );
    }
    #[test]
    fn state_ownership_and_schema_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let db = Repository::open(directory.path().join("state").as_path()).unwrap();
        assert!(matches!(
            Repository::open(directory.path().join("state").as_path()),
            Err(Error::Locked)
        ));
        db.db.pragma_update(None, "user_version", 999).unwrap();
        drop(db);
        assert!(matches!(
            Repository::open(directory.path().join("state").as_path()),
            Err(Error::Schema)
        ));
        let connection = Connection::open(
            directory
                .path()
                .join("state")
                .as_path()
                .join("custody.sqlite3"),
        )
        .unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            999
        );
        drop(connection);
        std::fs::write(
            directory
                .path()
                .join("state")
                .as_path()
                .join("custody.sqlite3"),
            b"corrupted fixture",
        )
        .unwrap();
        assert!(matches!(
            Repository::open(directory.path().join("state").as_path()),
            Err(Error::Schema)
        ));
    }
    #[test]
    fn capacity_never_discards_unprocessed_custody() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Repository::open(&dir.path().join("state")).unwrap();
        db.max_records = 1;
        let receipt = db.receive(&delivery(), 1).unwrap();
        let mut second = delivery();
        second.id = "second_request".into();
        assert!(matches!(db.receive(&second, 2), Err(Error::Capacity)));
        assert_eq!(db.receive(&delivery(), 3).unwrap(), receipt);
        db.max_records = 10;
        db.max_payload_bytes = 1;
        assert!(matches!(db.receive(&second, 4), Err(Error::Capacity)));
    }

    #[test]
    fn transaction_failure_rolls_back_binding_and_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let mut db = Repository::open(directory.path().join("state").as_path()).unwrap();
        db.db.execute_batch("CREATE TRIGGER fail_inbox BEFORE INSERT ON inbox BEGIN SELECT RAISE(ABORT,'fixture disk failure'); END;").unwrap();
        assert!(db.receive(&delivery(), 0).is_err());
        assert_eq!(
            db.db
                .query_row("SELECT COUNT(*) FROM bindings", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.db
                .query_row("SELECT COUNT(*) FROM inbox", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            0
        );
        db.db.execute_batch("DROP TRIGGER fail_inbox;").unwrap();
        assert!(db.receive(&delivery(), 0).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn stale_generation_and_linked_state_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        let mut db = Repository::open(directory.path().join("state").as_path()).unwrap();
        db.receive(&delivery(), 0).unwrap();
        let mut newer = delivery();
        newer.generation = 2;
        assert!(matches!(db.receive(&newer, 0), Err(Error::Generation)));
        drop(db);
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::remove_file(directory.path().join("state").as_path().join("owner.lock")).unwrap();
        std::os::unix::fs::symlink(
            outside.path(),
            directory.path().join("state").as_path().join("owner.lock"),
        )
        .unwrap();
        assert!(matches!(
            Repository::open(directory.path().join("state").as_path()),
            Err(Error::Private)
        ));
    }
}
