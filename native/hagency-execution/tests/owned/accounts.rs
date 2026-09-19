use super::*;
#[cfg(unix)]
#[tokio::test]
async fn native_local_codex_host() {
    for choice in [
        "selected",
        "preset",
        "seat",
        "managed",
        "replacement",
        "workspace",
        "during",
    ] {
        let f = Fixture::configured_account(false, choice == "managed");
        let root = f.root.path().canonicalize().unwrap();
        let home = root.join("provider-home");
        let codex = root.join("provider-codex");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&codex).unwrap();
        fs::write(codex.join("fixture-account-marker"), "selected-local").unwrap();
        let binding = hagency_execution::LocalCodex::new(
            if choice == "preset" {
                "foreign"
            } else {
                "pool"
            }
            .into(),
            if choice == "seat" { "foreign" } else { "seat" }.into(),
            home.clone(),
            if choice == "workspace" {
                f.work.clone()
            } else {
                codex.clone()
            },
        )
        .unwrap();
        let host = f
            .host(
                if choice == "during" {
                    "silent"
                } else {
                    "local-account"
                },
                "work",
                false,
            )
            .with_local_codex(binding)
            .unwrap();
        if choice == "replacement" {
            fs::rename(&codex, root.join("original-codex")).unwrap();
            fs::create_dir(&codex).unwrap();
        }
        let mut operation =
            Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
        if choice == "during" {
            f.entered().await;
            fs::rename(&codex, root.join("original-codex")).unwrap();
            fs::create_dir(&codex).unwrap();
        }
        let report = operation.wait().await.unwrap();
        match choice {
            "selected" => {
                assert_eq!(report.protocol, Protocol::Completed, "{:?}", report.failure);
                assert_eq!(report.failure, None);
                let observation: serde_json::Value = serde_json::from_slice(
                    &fs::read(f.work.join("local-account-observed.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    observation,
                    json!({"selected":true,"same_home":false,"ambient_key":false})
                );
                assert_eq!(f.count("SELECT COUNT(*) FROM managed_accounts"), 0);
                assert_eq!(
                    f.count("SELECT COUNT(*) FROM account_login_observations"),
                    0
                );
                assert_eq!(report.canonical_status, Some(TaskState::InProgress));
            }
            "during" => {
                assert_eq!(report.failure, Some(Failure::LostAuthority));
                f.quarantined();
            }
            _ => {
                assert_eq!(report.protocol, Protocol::NotStarted, "{choice}");
                assert!(!f.marker().exists(), "{choice}");
            }
        }
        assert!(fs::read_to_string(home.join("auth.json")).is_err());
        drop(report);
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_account_host_consumer() {
    for choice in [
        "selected",
        "other",
        "legacy",
        "retired",
        "replacement",
        "lost_caller",
    ] {
        let f = Fixture::configured_account(false, true);
        let ids = f.account_ids();
        let mut host = f.host("account", "work", false);
        if choice != "legacy" {
            let id = if choice == "other" { &ids[1] } else { &ids[0] };
            let account = f.domain.managed_account(id.clone()).await.unwrap();
            if choice == "retired" {
                account.retire();
            }
            host = host.with_managed_account(account).unwrap();
        }
        if choice == "replacement" {
            let original = f.root.path().join("state").join(&ids[0]);
            let renamed = fs::rename(&original, original.with_extension("retained-original"));
            if cfg!(windows) {
                // The retained directory handle refuses the rename itself on
                // Windows (ERROR_SHARING_VIOLATION); the namespace cannot be
                // replaced under the original handle, which is the property
                // the Unix branch proves through the mismatch below.
                let error = renamed.expect_err("Windows rename of a retained namespace");
                assert_eq!(error.raw_os_error(), Some(32), "{error}");
                assert_eq!(
                    fs::read_to_string(original.join("fixture-account-marker")).unwrap(),
                    "selected-A"
                );
                continue;
            }
            renamed.unwrap();
            hagency_store::private::create_directory_new(&original).unwrap();
            fs::write(original.join("fixture-account-marker"), "selected-A").unwrap();
        }
        let mut operation =
            Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
        if choice == "lost_caller" {
            drop(operation);
            assert_eq!(
                fs::read_to_string(
                    f.root
                        .path()
                        .join("state")
                        .join(&ids[0])
                        .join("fixture-account-marker")
                )
                .unwrap(),
                "selected-A"
            );
            continue;
        }
        let report = operation.wait().await.unwrap();
        if choice == "selected" {
            assert_eq!(report.protocol, Protocol::Completed);
            let observed: serde_json::Value =
                serde_json::from_slice(&fs::read(f.work.join("account-observed.json")).unwrap())
                    .unwrap();
            assert_eq!(
                observed,
                json!({"marker":"selected-A","same_home":true,"ambient_key":false})
            );
            let Cleanup::Observed(cleanup) = report.cleanup else {
                panic!("cleanup remains unknown")
            };
            assert!(cleanup.scope.leader_exited);
            if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
                assert!(report.retains_process_custody());
            } else {
                assert!(cleanup.scope.whole_tree_stopped);
            }
        } else {
            assert_eq!(report.protocol, Protocol::NotStarted, "{choice}");
            assert!(!f.work.join("account-observed.json").exists());
        }
    }
    // Retirement after actual child entry is observed by the existing authority
    // renewal schedule. The original process/namespace custody remains retained.
    let f = Fixture::configured_account(false, true);
    let id = f.account_ids().remove(0);
    let retirement = f.domain.managed_account(id.clone()).await.unwrap();
    let host = f
        .host("silent", "work", false)
        .with_managed_account(f.domain.managed_account(id).await.unwrap())
        .unwrap();
    let mut operation = Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
    f.entered().await;
    retirement.retire();
    let report = operation.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::LostAuthority));
    assert_eq!(report.protocol, Protocol::Unknown);
    if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        assert!(report.retains_process_custody());
    }
    f.quarantined();
}

/// MA-S2 (ADR-053 amendment) scenario 1's second Then: the Host admission
/// re-checks the same readiness predicate the selector uses, so a row the
/// selector would park is refused by the admission even when handed to it
/// directly — neither trusts the other's cache.
#[tokio::test]
async fn native_host_admission_refuses_unready_account() {
    let f = Fixture::configured_account(false, true);
    let bound = f
        .bound_account
        .clone()
        .expect("managed fixture exposes the bound account");
    // Shadow the seeded observed fact with a later uncertain fact: the latest
    // observation of any outcome decides, so readiness is now unknown even
    // though the claim already leased the dispatch.
    let shadow_at = now() + 5_000;
    f.sql()
        .execute(
            "INSERT INTO account_login_observations \
             (id,account_id,account_generation,attempt,observed_at_ms,expires_at_ms,mode,provider_state,outcome) \
             VALUES(?1,?2,1,2,?3,?4,'unknown','not-logged-in','uncertain')",
            rusqlite::params![
                format!("shadow_{shadow_at}"),
                bound,
                shadow_at,
                shadow_at + 3_600_000
            ],
        )
        .unwrap();
    let mut host = f.host("account", "work", false);
    let account = f.domain.managed_account(bound).await.unwrap();
    host = host.with_managed_account(account).unwrap();
    let mut operation = Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
    let report = operation.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::Admission));
    assert_eq!(report.protocol, Protocol::NotStarted);
    let reason: String = f
        .sql()
        .query_row(
            "SELECT park_reason FROM runner_attempts WHERE dispatch_id='dispatch' AND park_reason IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reason, "account_readiness_unknown");
}
