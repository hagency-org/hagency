use super::*;
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
            if cfg!(target_os = "macos") {
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
    if cfg!(target_os = "macos") {
        assert!(report.retains_process_custody());
    }
    f.quarantined();
}
