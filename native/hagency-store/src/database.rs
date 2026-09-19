//! Shared private-file and SQLite startup policy for explicitly named stores.
use crate::{Error, private};
use rusqlite::{Connection, TransactionBehavior};
use std::{fs::File, path::Path, time::Duration};

pub(crate) struct Database {
    pub connection: Connection,
    pub ownership: File,
}

pub(crate) struct Schema {
    pub name: &'static str,
    pub lock: &'static str,
    pub application_id: i32,
    pub version: i32,
    pub sql: &'static str,
    pub verify: &'static [&'static str],
    pub migrations: &'static [(i32, &'static str)],
}
pub(crate) fn open(directory: &Path, definition: Schema) -> Result<Database, Error> {
    let Schema {
        name: database_name,
        lock: lock_name,
        application_id,
        version: expected_version,
        sql: schema,
        verify: verify_sql,
        migrations,
    } = definition;
    private::directory(directory)?;
    let lock_path = directory.join(lock_name);
    let lock = match private::open(&lock_path, true) {
        Ok(lock) => lock,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            private::open(&lock_path, false)?
        }
        Err(e) => return Err(e),
    };
    lock.try_lock().map_err(|_| Error::Locked)?;
    let path = directory.join(database_name);
    let new = match private::open(&path, true) {
        Ok(_) => true,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            private::open(&path, false)?;
            false
        }
        Err(e) => return Err(e),
    };
    for suffix in ["-wal", "-shm", "-journal"] {
        let suffix = format!("{database_name}{suffix}");
        if directory.join(&suffix).symlink_metadata().is_ok() {
            private::open_journal(&directory.join(&suffix))?;
        }
    }
    let mut db = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.busy_timeout(Duration::from_millis(100))?;
    let version = if new {
        0
    } else {
        let id: i32 = db
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(|_| Error::Schema)?;
        let version: i32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| Error::Schema)?;
        if id != application_id || version < 1 || version > expected_version {
            return Err(Error::Schema);
        }
        let check: String = db
            .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
            .map_err(|_| Error::Schema)?;
        if check != "ok" {
            return Err(Error::Schema);
        }
        version
    };
    if version < expected_version {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut current = version;
        if new {
            tx.execute_batch(schema)?;
            tx.pragma_update(None, "application_id", application_id)?;
            current = 1;
        }
        while current < expected_version {
            let next = current + 1;
            let matching: Vec<_> = migrations
                .iter()
                .filter(|(version, _)| *version == next)
                .collect();
            if matching.len() != 1 {
                return Err(Error::Schema);
            }
            tx.execute_batch(matching[0].1)?;
            current = next;
        }
        for query in verify_sql {
            tx.prepare(query).map_err(|_| Error::Schema)?;
        }
        tx.pragma_update(None, "user_version", current)?;
        tx.commit()?;
    } else {
        for query in verify_sql {
            db.prepare(query).map_err(|_| Error::Schema)?;
        }
    }
    // The close path is a bounded resource release, not a checkpoint (ADR-120).
    // With checkpoints-on-close disabled SQLite skips the EXCLUSIVE lock and
    // the PASSIVE checkpoint at close and leaves both the -wal and the -shm in
    // place instead of unlinking them; committed frames stay in the WAL and
    // are replayed by the next open, which already admits leftover -wal, -shm
    // and -journal files above. Every commit is synced under synchronous=FULL,
    // so no committed data depends on the close-time checkpoint.
    db.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "synchronous", "FULL")?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    Ok(Database {
        connection: db,
        ownership: lock,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition(version: i32, migrations: &'static [(i32, &'static str)]) -> Schema {
        Schema {
            name: "fixture.sqlite3",
            lock: "fixture.lock",
            application_id: 123456,
            version,
            sql: "CREATE TABLE probe(id INTEGER PRIMARY KEY); INSERT INTO probe VALUES(1);",
            verify: &["SELECT id FROM probe LIMIT 0"],
            migrations,
        }
    }
    #[test]
    fn native_schema_migrations_are_atomic() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        drop(open(&state, definition(1, &[])).unwrap());
        let failing = &[(
            2,
            "ALTER TABLE probe ADD COLUMN upgraded INTEGER; INSERT INTO missing_table VALUES(1);",
        )];
        assert!(open(&state, definition(2, failing)).is_err());
        let db = open(&state, definition(1, &[])).unwrap();
        assert_eq!(
            db.connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            1
        );
        assert!(db.connection.prepare("SELECT upgraded FROM probe").is_err());
        assert_eq!(
            db.connection
                .query_row("SELECT COUNT(*) FROM probe", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            1
        );
        drop(db);
        let success = &[(
            2,
            "ALTER TABLE probe ADD COLUMN upgraded INTEGER; UPDATE probe SET upgraded=7;",
        )];
        let mut later_verification_failure = definition(2, success);
        later_verification_failure.verify = &[
            "SELECT id, upgraded FROM probe LIMIT 0",
            "SELECT missing_column FROM probe LIMIT 0",
        ];
        assert!(matches!(
            open(&state, later_verification_failure),
            Err(Error::Schema)
        ));
        // Even after migration SQL and the first verification succeed, a later
        // invalid statement must roll back the schema and its version together.
        let db = open(&state, definition(1, &[])).unwrap();
        assert_eq!(
            db.connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            1
        );
        assert!(db.connection.prepare("SELECT upgraded FROM probe").is_err());
        assert_eq!(
            db.connection
                .query_row("SELECT id FROM probe", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            1
        );
        drop(db);
        for _ in 0..2 {
            let mut complete_verification = definition(2, success);
            complete_verification.verify = &[
                "SELECT id FROM probe LIMIT 0",
                "SELECT upgraded FROM probe LIMIT 0",
            ];
            let db = open(&state, complete_verification).unwrap();
            assert_eq!(
                db.connection
                    .query_row("SELECT upgraded FROM probe", [], |r| r.get::<_, i32>(0))
                    .unwrap(),
                7
            );
            assert_eq!(
                db.connection
                    .pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                    .unwrap(),
                2
            );
        }
        // The already-current path must check every statement too, without
        // relying on a new migration to discover a malformed schema.
        let mut invalid_reopen = definition(2, &[]);
        invalid_reopen.verify = &[
            "SELECT id FROM probe LIMIT 0",
            "SELECT missing_column FROM probe LIMIT 0",
        ];
        assert!(matches!(open(&state, invalid_reopen), Err(Error::Schema)));
        assert!(matches!(
            open(&state, definition(1, &[])),
            Err(Error::Schema)
        ));
        // Exercise the actual domain1 -> domain2 migration with an existing pool.
        let old = root.path().join("old-domain");
        let database = open(
            &old,
            Schema {
                name: "domain.sqlite3",
                lock: "domain.lock",
                application_id: 0x48414732,
                version: 1,
                sql: include_str!("domain.sql"),
                verify: &["SELECT id FROM resources LIMIT 0"],
                migrations: &[],
            },
        )
        .unwrap();
        let config = r#"{"presetId":"pool","seatId":"seat","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium","roles":["architect"],"ceiling":{"tokens":100},"published":true}"#;
        database
            .connection
            .execute(
                "INSERT INTO resources(id,preset_id,config) VALUES(?1,'pool',?2)",
                rusqlite::params![hagency_core::project::public_resource_id("pool"), config],
            )
            .unwrap();
        drop(database);
        let mut domain = crate::DomainRepository::open(&old).unwrap();
        let catalog = domain.catalog("", 100).unwrap();
        assert_eq!(catalog.len(), 1);
        assert!(catalog[0].roles.contains(&"coding".into()));
        assert!(!catalog[0].roles.contains(&"architect".into()));
        domain.set_role_publication("coding", false).unwrap();
        drop(domain);
        assert!(
            !crate::DomainRepository::open(&old)
                .unwrap()
                .catalog("", 100)
                .unwrap()[0]
                .roles
                .contains(&"coding".into())
        );
    }
}
