#[path = "bootstrap/accounts.rs"]
mod accounts;
#[path = "bootstrap/approval.rs"]
mod approval;
#[path = "bootstrap/fixture.rs"]
mod fixture;
#[path = "bootstrap/scope.rs"]
mod scope;
#[path = "support/approval_enrollment.rs"]
mod support;
use fixture::*;
use serde_json::json;

#[tokio::test]
async fn native_configured_fleet_profile() {
    use hagency_store::private;
    for kind in [
        "token",
        "appservice",
        "missing_approval",
        "missing_home",
        "account_only",
        "profile",
        "extra",
        "idle_zero",
        "idle_large",
        "foreign_approval",
        "no_intake",
    ] {
        let f = Fixture::new(false).await;
        let base = f.root.path().canonicalize().unwrap();
        let homes = base.join("fleet-homes");
        let source = base.join("fleet-source");
        private::directory(&homes).unwrap();
        private::directory(&source).unwrap();
        let mut config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(f.state_dir.join("development-driver.json")).unwrap(),
        )
        .unwrap();
        let registration = hagency_store::DomainRepository::open(&f.state_dir)
            .unwrap()
            .provisioning_registration_for_engagement(
                config["matrix"]["engagement_id"].as_str().unwrap(),
            )
            .unwrap();
        let fingerprint = hagency_core::canonical::digest(&json!(&registration)).unwrap();
        let peer = support::crypto::Peer::new().await;
        let anchors = json!([{"user_id":"@owner:example.test","master_key":peer.anchor()}]);
        config["profile"] = json!("codex_app_server_agent_v1");
        config["intake_sessions"] = json!(["session"]);
        config["factory_service"] =
            json!({"profile":"inline_factory_service_checkpoint_v1","idle_ms":10_000});
        config["matrix"]["registration_fingerprint"] = json!(fingerprint);
        config["matrix"]["token_provisioning"] = json!({"profile":"registration_token_home_rooms_enrollment_step_v1","peer_masters":anchors,
            "home":{"root":homes,"task_client":std::path::PathBuf::from(env!("CARGO_BIN_EXE_hagency")).canonicalize().unwrap(),
                "projects":[{"project_id":"project_provision","source":source,"mode":"copy"}]}});
        config["approval"] = json!({"origin":f.fake.endpoint,"server_name":registration.server_name,"registration_fingerprint":fingerprint,
            "engagement_id":config["matrix"]["engagement_id"],"registration_generation":1,"transport_generation":1,
            "sender_mxid":registration.approval_bot_mxid,"device_id":"APPROVAL_DEVICE",
            "rooms":[{"id":"!private:example.test","generation":1,"privacy":{"kind":"direct","human_mxid":"@owner:example.test"}}],"peer_masters":anchors});
        match kind {
            "appservice" => {
                config["matrix"]["token_provisioning"]["profile"] =
                    json!("appservice_login_home_rooms_enrollment_step_v1");
                config["matrix"]["token_provisioning"]["namespace_prefix"] =
                    json!(format!("{}_", registration.fleet_id));
            }
            "missing_approval" => {
                config.as_object_mut().unwrap().remove("approval");
            }
            "missing_home" => {
                config["matrix"]["token_provisioning"]
                    .as_object_mut()
                    .unwrap()
                    .remove("home");
            }
            "account_only" => {
                config["matrix"]["token_provisioning"] =
                    json!({"profile":"registration_token_account_step_v1"})
            }
            "profile" => config["factory_service"]["profile"] = json!("complete_native_fleet"),
            "extra" => config["factory_service"]["ready"] = json!(true),
            "idle_zero" => config["factory_service"]["idle_ms"] = json!(0),
            "idle_large" => config["factory_service"]["idle_ms"] = json!(1_200_001),
            "foreign_approval" => {
                config["approval"]["registration_fingerprint"] = json!("0".repeat(64))
            }
            "no_intake" => config["intake_sessions"] = json!([]),
            _ => {}
        }
        private::write_new(
            &f.state_dir.join("agent-driver.json"),
            &serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        for (name, bytes) in [
            (
                "matrix.registration_token",
                b"synthetic-registration-token".as_slice(),
            ),
            (
                "matrix.appservice_token",
                b"synthetic-fixed-side-token".as_slice(),
            ),
            (
                "matrix.representative_token",
                b"synthetic-separate-representative".as_slice(),
            ),
            ("matrix.provisioning_key", &[73; 32]),
            (
                "approval.access_token",
                b"synthetic-approval-token".as_slice(),
            ),
            ("approval.sdk_key", &[85; 32]),
        ] {
            private::write_new(&f.state_dir.join(name), bytes).unwrap();
        }
        let result = hagency::bootstrap::Bootstrap::open_with_options(
            &f.state_dir,
            f.address,
            16,
            hagency::bootstrap::Options {
                agent_driver: true,
                ..Default::default()
            },
        );
        if matches!(kind, "token" | "appservice") {
            let mut service = result.unwrap();
            service.close().await.unwrap();
            assert!(f.state_dir.join("factory-task-contexts").is_dir());
        } else {
            assert!(
                matches!(result, Err(hagency::bootstrap::Failure::Config)),
                "{kind}"
            );
        }
        assert_eq!(
            f.fake.requests(),
            0,
            "configuration must not create physical/Matrix effects"
        );
        assert_eq!(f.attempts(), 0);
        assert_eq!(std::fs::read_dir(homes).unwrap().count(), 0);
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_bootstrap_token_provisioning_profile() {
    use hagency_store::private;
    use std::io::{Seek, Write};
    for kind in [
        "valid",
        "profile",
        "extra",
        "missing_token",
        "missing_key",
        "short_key",
        "token",
        "fingerprint",
    ] {
        let mut f = Fixture::new(false).await;
        let path = f.state_dir.join("development-driver.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let registration = hagency_store::DomainRepository::open(&f.state_dir)
            .unwrap()
            .provisioning_registration_for_engagement(
                config["matrix"]["engagement_id"].as_str().unwrap(),
            )
            .unwrap();
        config["matrix"]["registration_fingerprint"] =
            json!(hagency_core::canonical::digest(&json!(&registration)).unwrap());
        config["matrix"]["token_provisioning"] =
            json!({"profile":"registration_token_account_step_v1"});
        if kind == "profile" {
            config["matrix"]["token_provisioning"]["profile"] = json!("application_service");
        }
        if kind == "extra" {
            config["matrix"]["token_provisioning"]["token"] = json!("must-not-enter-driver-json");
        }
        if kind == "fingerprint" {
            config["matrix"]["registration_fingerprint"] = json!("0".repeat(64));
        }
        if kind != "missing_token" {
            private::write_new(
                &f.state_dir.join("matrix.registration_token"),
                if kind == "token" {
                    b"invalid token"
                } else {
                    b"synthetic-registration-token"
                },
            )
            .unwrap();
        }
        if kind != "missing_key" {
            private::write_new(
                &f.state_dir.join("matrix.provisioning_key"),
                if kind == "short_key" {
                    &[73; 31]
                } else {
                    &[73; 32]
                },
            )
            .unwrap();
        }
        let mut file = private::open(&path, false).unwrap();
        file.set_len(0).unwrap();
        file.rewind().unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        drop(file);
        if kind == "valid" {
            let child = f.launch(true);
            let first = f.fake.next().await;
            assert_eq!(first.target, "/_matrix/client/v3/account/whoami");
            assert_eq!(
                first.headers["authorization"],
                format!("Bearer {}", common::TOKEN)
            );
            // Actual valid private profile reaches the existing authenticated
            // refresh; wrong whoami prevents any model/task or account effect.
            first.json(
                200,
                json!({"user_id":"@wrong:example.test", "device_id":"WRONG"}),
            );
            let status = f.wait_result().await;
            assert_eq!(status["error"], "refresh");
            let capabilities = f.capabilities().await.to_string();
            assert!(!capabilities.contains("synthetic-registration-token"));
            assert_eq!(f.fake.requests(), 1);
            assert_eq!(f.attempts(), 0);
            drop(child);
        } else {
            let mut command = tokio::process::Command::from(f.command(true));
            command
                .kill_on_drop(true)
                .stderr(std::process::Stdio::piped());
            let result = tokio::time::timeout(STARTUP_WATCHDOG, command.output())
                .await
                .unwrap()
                .unwrap();
            assert!(!result.status.success(), "{kind}");
            let stderr = String::from_utf8_lossy(&result.stderr);
            assert!(stderr.contains("Error: Config"), "{kind}");
            assert!(!stderr.contains("synthetic-registration-token"));
            assert_eq!(f.attempts(), 0);
            assert_eq!(f.fake.requests(), 0);
        }
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_bootstrap_token_rooms_profile() {
    use hagency_store::private;
    use std::io::{Seek, Write};
    const REP: &[u8] = b"synthetic-separate-representative-token";
    for kind in [
        "valid",
        "missing_rep",
        "short_rep",
        "space_rep",
        "oversized_rep",
        "secret_json",
        "empty_anchors",
        "duplicate_anchors",
        "invalid_anchor",
        "invalid_mxid",
        "account_anchors",
        "account_null_anchors",
        "extra_anchor",
        "wrong_profile",
        "missing_anchors",
        "null_anchors",
        "invalid_point",
        "padded_anchor",
    ] {
        let mut f = Fixture::new(false).await;
        let path = f.state_dir.join("development-driver.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let registration = hagency_store::DomainRepository::open(&f.state_dir)
            .unwrap()
            .provisioning_registration_for_engagement(
                config["matrix"]["engagement_id"].as_str().unwrap(),
            )
            .unwrap();
        config["matrix"]["registration_fingerprint"] =
            json!(hagency_core::canonical::digest(&json!(&registration)).unwrap());
        let peer = support::crypto::Peer::new().await;
        let public = peer.anchor();
        assert!(
            hagency_matrix::TokenProvisioningHost::new(
                registration.clone(),
                config["matrix"]["origin"].as_str().unwrap(),
                "synthetic-registration-token",
                f.state_dir.clone(),
                [73; 32],
                hagency_matrix::Limits::default()
            )
            .unwrap()
            .with_agent_rooms_enrollment(
                std::str::from_utf8(REP).unwrap(),
                vec![("@owner:example.test".into(), public.clone())]
            )
            .is_ok()
        );
        let anchor = json!({"user_id":"@owner:example.test","master_key":public.clone()});
        config["matrix"]["token_provisioning"] = json!({"profile":"registration_token_rooms_enrollment_step_v1","peer_masters":[anchor.clone()]});
        let profile = &mut config["matrix"]["token_provisioning"];
        match kind {
            "secret_json" => profile["representative_token"] = json!("must-not-enter-driver-json"),
            "empty_anchors" => profile["peer_masters"] = json!([]),
            "duplicate_anchors" => profile["peer_masters"] = json!([anchor.clone(), anchor]),
            "invalid_anchor" => profile["peer_masters"][0]["master_key"] = json!("invalid"),
            "invalid_mxid" => profile["peer_masters"][0]["user_id"] = json!("owner"),
            "account_anchors" => profile["profile"] = json!("registration_token_account_step_v1"),
            "account_null_anchors" => {
                profile["profile"] = json!("registration_token_account_step_v1");
                profile["peer_masters"] = serde_json::Value::Null;
            }
            "extra_anchor" => profile["peer_masters"][0]["trusted"] = json!(true),
            "wrong_profile" => profile["profile"] = json!("application_service"),
            "missing_anchors" => {
                profile.as_object_mut().unwrap().remove("peer_masters");
            }
            "null_anchors" => profile["peer_masters"] = serde_json::Value::Null,
            "invalid_point" => {
                profile["peer_masters"][0]["master_key"] =
                    json!("QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE")
            }
            "padded_anchor" => {
                profile["peer_masters"][0]["master_key"] = json!(format!("{public}="))
            }
            _ => {}
        }
        private::write_new(
            &f.state_dir.join("matrix.registration_token"),
            b"synthetic-registration-token",
        )
        .unwrap();
        private::write_new(&f.state_dir.join("matrix.provisioning_key"), &[73; 32]).unwrap();
        if kind != "missing_rep" {
            let token: &[u8] = match kind {
                "short_rep" => b"short",
                "space_rep" => b"invalid representative token",
                _ => REP,
            };
            let oversized = vec![b'A'; 4097];
            private::write_new(
                &f.state_dir.join("matrix.representative_token"),
                if kind == "oversized_rep" {
                    &oversized
                } else {
                    token
                },
            )
            .unwrap();
        }
        let bytes = serde_json::to_vec(&config).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(std::str::from_utf8(REP).unwrap()));
        let mut file = private::open(&path, false).unwrap();
        file.set_len(0).unwrap();
        file.rewind().unwrap();
        file.write_all(&bytes).unwrap();
        drop(file);
        if kind == "valid" {
            let child = f.launch(true);
            let first = f.fake.next().await;
            assert_eq!(first.target, "/_matrix/client/v3/account/whoami");
            assert_eq!(
                first.headers["authorization"],
                format!("Bearer {}", common::TOKEN)
            );
            first.json(
                200,
                json!({"user_id":"@wrong:example.test","device_id":"WRONG"}),
            );
            assert_eq!(f.wait_result().await["error"], "refresh");
            let capabilities = f.capabilities().await.to_string();
            assert!(!capabilities.contains(std::str::from_utf8(REP).unwrap()));
            assert!(!capabilities.contains("synthetic-registration-token"));
            assert_eq!(f.fake.requests(), 1);
            assert_eq!(f.attempts(), 0);
            drop(child);
        } else {
            let mut command = tokio::process::Command::from(f.command(true));
            command
                .kill_on_drop(true)
                .stderr(std::process::Stdio::piped());
            let result = tokio::time::timeout(STARTUP_WATCHDOG, command.output())
                .await
                .unwrap()
                .unwrap();
            assert!(!result.status.success(), "{kind}");
            let stderr = String::from_utf8_lossy(&result.stderr);
            assert!(stderr.contains("Error: Config"), "{kind}");
            assert!(!stderr.contains(std::str::from_utf8(REP).unwrap()));
            assert_eq!(f.fake.requests(), 0);
            assert_eq!(f.attempts(), 0);
        }
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_bootstrap_token_home_profile() {
    use hagency_store::private;
    use std::io::{Seek, Write};
    for kind in [
        "valid",
        "missing_home",
        "extra_home",
        "mode",
        "duplicate",
        "missing_source",
        "nested_state",
        "source_state",
        "binary",
        "missing_rep",
        "profile",
        "root_alias",
    ] {
        let mut f = Fixture::new(false).await;
        let base = f.root.path().canonicalize().unwrap();
        let homes = base.join("homes");
        let source = base.join("declared_project");
        private::directory(&homes).unwrap();
        private::directory(&source).unwrap();
        private::write_new(&source.join("original.md"), b"original declared project").unwrap();
        let path = f.state_dir.join("development-driver.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let registration = hagency_store::DomainRepository::open(&f.state_dir)
            .unwrap()
            .provisioning_registration_for_engagement(
                config["matrix"]["engagement_id"].as_str().unwrap(),
            )
            .unwrap();
        let peer = support::crypto::Peer::new().await;
        config["matrix"]["registration_fingerprint"] =
            json!(hagency_core::canonical::digest(&json!(&registration)).unwrap());
        let project = json!({"project_id":"project_provision","source":source,"mode":"copy"});
        config["matrix"]["token_provisioning"] = json!({"profile":"registration_token_home_rooms_enrollment_step_v1",
            "peer_masters":[{"user_id":"@owner:example.test","master_key":peer.anchor()}],
            "home":{"root":homes,"task_client":std::path::PathBuf::from(env!("CARGO_BIN_EXE_hagency")).canonicalize().unwrap(),"projects":[project.clone()]}});
        let profile = &mut config["matrix"]["token_provisioning"];
        match kind {
            "missing_home" => {
                profile.as_object_mut().unwrap().remove("home");
            }
            "extra_home" => {
                profile["home"]["runner_capability"] = json!("must-not-enter-home-config")
            }
            "mode" => profile["home"]["projects"][0]["mode"] = json!("follow_external"),
            "duplicate" => profile["home"]["projects"] = json!([project.clone(), project]),
            "missing_source" => {
                profile["home"]["projects"][0]["source"] = json!(base.join("missing"))
            }
            "nested_state" => profile["home"]["root"] = json!(f.state_dir.canonicalize().unwrap()),
            "source_state" => {
                profile["home"]["projects"][0]["source"] =
                    json!(f.state_dir.canonicalize().unwrap())
            }
            "binary" => profile["home"]["task_client"] = json!(source.join("original.md")),
            "profile" => profile["profile"] = json!("registration_token_rooms_enrollment_step_v1"),
            "root_alias" => {
                #[cfg(unix)]
                {
                    let alias = base.join("homes_alias");
                    std::os::unix::fs::symlink(&homes, &alias).unwrap();
                    profile["home"]["root"] = json!(alias);
                }
                #[cfg(not(unix))]
                {
                    continue;
                }
            }
            _ => {}
        }
        private::write_new(
            &f.state_dir.join("matrix.registration_token"),
            b"synthetic-registration-token",
        )
        .unwrap();
        private::write_new(&f.state_dir.join("matrix.provisioning_key"), &[73; 32]).unwrap();
        if kind != "missing_rep" {
            private::write_new(
                &f.state_dir.join("matrix.representative_token"),
                b"synthetic-separate-representative-token",
            )
            .unwrap();
        }
        let mut file = private::open(&path, false).unwrap();
        file.set_len(0).unwrap();
        file.rewind().unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        drop(file);
        if kind == "valid" {
            let child = f.launch(true);
            let first = f.fake.next().await;
            assert_eq!(first.target, "/_matrix/client/v3/account/whoami");
            first.json(
                200,
                json!({"user_id":"@wrong:example.test","device_id":"WRONG"}),
            );
            assert_eq!(f.wait_result().await["error"], "refresh");
            assert_eq!(
                std::fs::read_dir(&homes).unwrap().count(),
                0,
                "configuration is not home fulfillment"
            );
            assert_eq!(f.attempts(), 0);
            assert_eq!(f.fake.requests(), 1);
            drop(child);
        } else {
            let mut command = tokio::process::Command::from(f.command(true));
            command
                .kill_on_drop(true)
                .stderr(std::process::Stdio::piped());
            let result = tokio::time::timeout(STARTUP_WATCHDOG, command.output())
                .await
                .unwrap()
                .unwrap();
            assert!(!result.status.success(), "{kind}");
            let stderr = String::from_utf8_lossy(&result.stderr);
            assert!(stderr.contains("Error: Config"), "{kind}");
            assert!(!stderr.contains("synthetic-separate-representative-token"));
            assert_eq!(f.attempts(), 0);
            assert_eq!(f.fake.requests(), 0);
        }
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_bootstrap_appservice_home_profile() {
    use hagency_store::private;
    use std::io::{Seek, Write};
    const AS: &[u8] = b"synthetic-fixed-AS+side/credential=";
    const REP: &[u8] = b"synthetic-separate-representative-token";
    for kind in [
        "valid",
        "valid_long",
        "default_untouched",
        "missing_as",
        "short_as",
        "space_as",
        "oversized_as",
        "wrong_namespace",
        "broad_namespace",
        "missing_namespace",
        "secret_json",
        "missing_home",
        "extra_home",
        "missing_anchors",
        "empty_anchors",
        "missing_rep",
        "missing_key",
        "wrong_profile",
        "ordinary_marker",
        "fingerprint",
        "as_alias",
        "public_as",
    ] {
        let mut f = Fixture::new(false).await;
        let base = f.root.path().canonicalize().unwrap();
        let homes = base.join("homes");
        let source = base.join("declared_project");
        private::directory(&homes).unwrap();
        private::directory(&source).unwrap();
        private::write_new(&source.join("original.md"), b"original declared project").unwrap();
        let path = f.state_dir.join("development-driver.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let registration = hagency_store::DomainRepository::open(&f.state_dir)
            .unwrap()
            .provisioning_registration_for_engagement(
                config["matrix"]["engagement_id"].as_str().unwrap(),
            )
            .unwrap();
        let peer = support::crypto::Peer::new().await;
        config["matrix"]["registration_fingerprint"] =
            json!(hagency_core::canonical::digest(&json!(&registration)).unwrap());
        config["matrix"]["token_provisioning"] = json!({"profile":"appservice_login_home_rooms_enrollment_step_v1",
            "namespace_prefix":format!("{}_",registration.fleet_id),
            "peer_masters":[{"user_id":"@owner:example.test","master_key":peer.anchor()}],
            "home":{"root":homes,"task_client":std::path::PathBuf::from(env!("CARGO_BIN_EXE_hagency")).canonicalize().unwrap(),
                "projects":[{"project_id":"project_provision","source":source,"mode":"copy"}]}});
        let profile = &mut config["matrix"]["token_provisioning"];
        match kind {
            "wrong_namespace" => profile["namespace_prefix"] = json!("different_side_"),
            "broad_namespace" => profile["namespace_prefix"] = json!("hf_"),
            "missing_namespace" => {
                profile.as_object_mut().unwrap().remove("namespace_prefix");
            }
            "secret_json" => profile["appservice_token"] = json!("must-not-enter-driver-json"),
            "missing_home" => {
                profile.as_object_mut().unwrap().remove("home");
            }
            "extra_home" => {
                profile["home"]["runner_capability"] = json!("must-not-enter-home-config")
            }
            "missing_anchors" => {
                profile.as_object_mut().unwrap().remove("peer_masters");
            }
            "empty_anchors" => profile["peer_masters"] = json!([]),
            "wrong_profile" => profile["profile"] = json!("application_service"),
            "ordinary_marker" => *profile = json!({"profile":"registration_token_account_step_v1"}),
            "default_untouched" => {
                config["matrix"]
                    .as_object_mut()
                    .unwrap()
                    .remove("token_provisioning");
            }
            "fingerprint" => config["matrix"]["registration_fingerprint"] = json!("0".repeat(64)),
            _ => {}
        }
        // An AS profile must neither require nor fall back to the ordinary
        // registration token. The ordinary marker still requires its own file.
        if kind != "ordinary_marker" {
            private::write_new(
                &f.state_dir.join("matrix.registration_token"),
                if kind == "missing_as" {
                    b"synthetic-registration-token"
                } else {
                    b"ignored invalid ordinary token"
                },
            )
            .unwrap();
        }
        let long = vec![b'+'; 4096];
        let oversized = vec![b'A'; 4097];
        if kind != "missing_as" {
            let token: &[u8] = match kind {
                "short_as" => b"short",
                "space_as" => b"invalid AS credential token",
                "valid_long" => &long,
                "oversized_as" => &oversized,
                _ => AS,
            };
            if kind == "as_alias" {
                #[cfg(unix)]
                {
                    let actual = f.state_dir.join("actual-private-AS-token");
                    private::write_new(&actual, token).unwrap();
                    std::os::unix::fs::symlink(
                        &actual,
                        f.state_dir.join("matrix.appservice_token"),
                    )
                    .unwrap();
                }
                #[cfg(not(unix))]
                {
                    continue;
                }
            } else {
                private::write_new(&f.state_dir.join("matrix.appservice_token"), token).unwrap();
            }
            if kind == "public_as" {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        f.state_dir.join("matrix.appservice_token"),
                        std::fs::Permissions::from_mode(0o644),
                    )
                    .unwrap();
                }
                #[cfg(not(unix))]
                {
                    continue;
                }
            }
        }
        if kind != "missing_key" {
            private::write_new(&f.state_dir.join("matrix.provisioning_key"), &[73; 32]).unwrap();
        }
        if kind != "missing_rep" {
            private::write_new(&f.state_dir.join("matrix.representative_token"), REP).unwrap();
        }
        let bytes = serde_json::to_vec(&config).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(std::str::from_utf8(AS).unwrap()));
        assert!(!String::from_utf8_lossy(&bytes).contains(std::str::from_utf8(REP).unwrap()));
        let mut file = private::open(&path, false).unwrap();
        file.set_len(0).unwrap();
        file.rewind().unwrap();
        file.write_all(&bytes).unwrap();
        drop(file);
        if matches!(kind, "valid" | "valid_long" | "default_untouched") {
            let child = f.launch(true);
            let first = f.fake.next().await;
            assert_eq!(first.target, "/_matrix/client/v3/account/whoami");
            assert_eq!(
                first.headers["authorization"],
                format!("Bearer {}", common::TOKEN)
            );
            first.json(
                200,
                json!({"user_id":"@wrong:example.test","device_id":"WRONG"}),
            );
            assert_eq!(f.wait_result().await["error"], "refresh");
            assert_eq!(
                std::fs::read_dir(&homes).unwrap().count(),
                0,
                "configuration is not fulfillment"
            );
            let capabilities = f.capabilities().await.to_string();
            assert!(
                !capabilities.contains(std::str::from_utf8(AS).unwrap())
                    && !capabilities.contains(std::str::from_utf8(REP).unwrap())
            );
            assert_eq!(f.attempts(), 0);
            assert_eq!(f.fake.requests(), 1);
            drop(child);
        } else {
            let mut command = tokio::process::Command::from(f.command(true));
            command
                .kill_on_drop(true)
                .stderr(std::process::Stdio::piped());
            let result = tokio::time::timeout(STARTUP_WATCHDOG, command.output())
                .await
                .unwrap()
                .unwrap();
            assert!(!result.status.success(), "{kind}");
            let stderr = String::from_utf8_lossy(&result.stderr);
            assert!(stderr.contains("Error: Config"), "{kind}");
            assert!(
                !stderr.contains(std::str::from_utf8(AS).unwrap())
                    && !stderr.contains(std::str::from_utf8(REP).unwrap())
            );
            assert_eq!(f.attempts(), 0);
            assert_eq!(f.fake.requests(), 0);
        }
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_bootstrap_executable() {
    let mut f = Fixture::new(false).await;
    let child = f.launch(true);
    let first = f.fake.next().await;
    assert_eq!(f.state(), "queued");
    assert_eq!(f.attempts(), 0);
    assert!(!f.work.join("owned-mcp.requests").exists());
    first.json(200, common::who());
    f.fake.next().await.json(200, common::sync("bootstrap"));
    f.fake.next().await.json(200, common::state());
    f.fake.next().await.json(200, common::state());
    let status = f.wait_result().await;
    assert_eq!(status["mode"], "one_attempt");
    assert_eq!(status["workspace_registered"], true);
    assert_eq!(status["protocol"], "completed");
    assert_eq!(f.attempts(), 1);
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.work.join("owned-mcp.receipt")).unwrap()).unwrap();
    assert_eq!(receipt["task"]["id"], "task");
    assert_eq!(receipt["task"]["status"], "in_progress");
    assert_eq!(receipt["helper_exit"], true);
    let capability = f.capabilities().await;
    assert_eq!(capability["agent_execution"], false);
    assert_eq!(capability["production_api_parity"], false);
    let safe = capability.to_string();
    assert!(!safe.contains(common::TOKEN));
    assert!(!safe.contains(f.work.to_str().unwrap()));
    // No automatic second claim even though this peer returned its terminal update.
    assert_eq!(f.attempts(), 1);
    drop(child);
}

/// Linux supplies the whole-tree cleanup proof needed before the next claim.
/// This drives two real owned operations through one retained host, including
/// a fresh authenticated Matrix refresh between them and an exact workspace
/// handoff release. macOS deliberately cannot claim this proof yet.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn native_continuous_driver_runs_two_sequential_dispatches() {
    let mut f = Fixture::new(false).await;
    f.enqueue_second();
    f.configure_continuous();
    let mut child = f.launch_continuous();

    common::success(&mut f.fake, "continuous-1").await;
    f.fake.next().await.json(200, common::state());
    f.wait_attempts(1).await;

    common::success(&mut f.fake, "continuous-2").await;
    f.fake.next().await.json(200, common::state());
    f.wait_attempts(2).await;
    let until = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    let receipt = loop {
        if let Ok(bytes) = std::fs::read(f.work.join("owned-mcp.receipt"))
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && value["task"]["id"] == "task-2"
        {
            break value;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "second operation did not publish its fixture receipt"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    assert_eq!(receipt["task"]["id"], "task-2");
    assert_eq!(
        f.capabilities().await["development_execution"]["mode"],
        "continuous"
    );

    child.request_shutdown();
    child.exited().await;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn native_continuous_driver_operator_resolution() {
    use hagency_store::private;
    use sha2::{Digest, Sha256};
    async fn post(
        client: &reqwest::Client,
        origin: &str,
        path: &str,
        body: serde_json::Value,
        cookie: Option<&str>,
    ) -> (serde_json::Value, Option<String>) {
        let mut request = client
            .post(format!("{origin}{path}"))
            .header("Content-Type", "application/json")
            .header("Origin", origin)
            .header("Sec-Fetch-Site", "same-origin")
            .body(serde_json::to_vec(&body).unwrap());
        if let Some(cookie) = cookie {
            request = request.header("Cookie", cookie);
        }
        let response = request.send().await.unwrap();
        let cookie = response
            .headers()
            .get("set-cookie")
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned());
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        assert_eq!(status, reqwest::StatusCode::OK, "console request refused");
        (serde_json::from_slice(&bytes).unwrap(), cookie)
    }
    let mut f = Fixture::new(false).await;
    std::fs::write(
        f.work.join("owned-mcp.fail-notification"),
        b"offline fixture",
    )
    .unwrap();
    f.configure_continuous();
    let assets = f.root.path().join("console");
    private::directory(&assets).unwrap();
    let bytes = b"<!doctype html><html><body>offline console fixture</body></html>";
    let mut manifest = Vec::new();
    for page in ["usage", "engagements"] {
        std::fs::create_dir(assets.join(page)).unwrap();
        private::write_new(&assets.join(page).join("index.html"), bytes).unwrap();
        manifest.push(json!({"path":format!("{page}/index.html"),"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes)),"mime":"text/html; charset=utf-8"}));
    }
    private::write_new(
        &assets.join("manifest.json"),
        json!({"version":1,"assets":manifest})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let file = private::open(&f.root.path().join("native.stderr"), true).unwrap();
    let mut child = Running::from_child(
        f.command(false)
            .arg("--agent-driver")
            .arg("--console-assets")
            .arg(assets.canonicalize().unwrap())
            .stderr(std::process::Stdio::from(file))
            .spawn()
            .unwrap(),
    );
    f.capabilities().await;
    common::success(&mut f.fake, "operator-resolution-1").await;
    f.fake.next().await.json(200, common::state());
    let until = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let status = f.capabilities().await;
        if status["development_execution"]["state"] == "outcome_unknown" {
            assert_eq!(
                status["development_execution"]["cleanup"],
                "whole_tree_stopped"
            );
            assert_eq!(
                status["development_execution"]["runtime"]["refused_notification"],
                "unknown"
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "original process did not fail"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.state_dir.join("agent-driver.json")).unwrap())
            .unwrap();
    let engagement = config["matrix"]["engagement_id"].as_str().unwrap();
    let link = hagency::console::client::lifecycle_access(&f.state_dir, f.address)
        .await
        .unwrap();
    let ticket = link.split_once("#access=").unwrap().1;
    let origin = format!("http://{}", f.address);
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let (_, cookie) = post(
        &client,
        &origin,
        "/console/session",
        json!({"ticket":ticket}),
        None,
    )
    .await;
    let cookie = cookie.unwrap();
    let (inspection, _) = post(
        &client,
        &origin,
        &format!("/console/api/agents/{engagement}/stopped-dispatches/dispatch/inspect"),
        json!({}),
        Some(&cookie),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(f.attempts(), 1, "stopped proof alone cannot rerun work");
    // ADR-181: the failed attempt left its record — the uncollapsed status as
    // the `failed` event, and the retained product's terminal_reason shape,
    // `<failure>:<exit identity>:<stderr tail>`, with the clock set.
    {
        let sql = f.sql();
        let failed: String = sql
            .query_row(
                "SELECT detail FROM runner_attempt_events WHERE dispatch_id='dispatch' AND fence=1 AND phase='failed'",
                [],
                |r| r.get(0),
            )
            .expect("the failed attempt is recorded");
        let failed: serde_json::Value = serde_json::from_str(&failed).unwrap();
        assert_eq!(failed["status"]["owned_failure"], "protocol", "{failed}");
        assert_eq!(failed["status"]["state"], "outcome_unknown");
        let (reason, started_at, settled_at): (String, Option<u64>, Option<u64>) = sql
            .query_row(
                "SELECT terminal_reason, started_at, settled_at FROM runner_attempts WHERE dispatch_id='dispatch' AND fence=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        // The host ends the refused runtime itself, so the exit identity is
        // the stop signal; a runtime that exits on its own reads `code:N`.
        assert!(
            reason.starts_with("protocol:signal:") || reason.starts_with("protocol:code:"),
            "{reason}"
        );
        assert!(
            started_at.is_some() && settled_at >= started_at,
            "{started_at:?} {settled_at:?}"
        );
        let phases: Vec<String> = sql
            .prepare("SELECT phase FROM runner_attempt_events WHERE dispatch_id='dispatch' AND fence=1 ORDER BY seq")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            phases.first().map(String::as_str),
            Some("claimed"),
            "{phases:?}"
        );
        assert_eq!(
            phases.last().map(String::as_str),
            Some("failed"),
            "{phases:?}"
        );
    }
    std::fs::remove_file(f.work.join("owned-mcp.fail-notification")).unwrap();
    post(&client,&origin,&format!("/console/api/agents/{engagement}/resolve-stopped-dispatch"),json!({
        "original":"dispatch","requestId":"reviewed_resolution","inspectionId":inspection["inspectionId"],"inspectionToken":inspection["inspectionToken"],
        "action":"continue","operatorNote":"Reviewed original failed subprocess inventory",
        "replacement":{"id":"reviewed_continuation","session_id":"session","task_id":"task",
            "resources":[{"id":"work","exclusive":true}],"payload":{"instruction":"Inspect prior state and heartbeat the recovered task"}},
    }),Some(&cookie)).await;
    common::success(&mut f.fake, "operator-resolution-2").await;
    f.fake.next().await.json(200, common::state());
    f.wait_attempts(2).await;
    let until = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Ok(bytes) = std::fs::read(f.work.join("owned-mcp.receipt"))
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && value["task"]["id"] == "task"
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "continuation helper did not run"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        f.state(),
        "outcome_unknown",
        "original attempt is preserved"
    );
    let sql = rusqlite::Connection::open(f.state_dir.join("domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM runner_attempts WHERE dispatch_id='dispatch'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM runner_attempts WHERE dispatch_id='reviewed_continuation'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
    drop(sql);
    child.request_shutdown();
    child.exited().await;
}

#[tokio::test]
async fn native_bootstrap_refresh_refusal() {
    for failure in ["identity", "room", "fenced"] {
        let mut f = Fixture::new(failure == "fenced").await;
        let child = f.launch(true);
        if failure != "fenced" {
            f.fake.next().await.json(
                200,
                if failure == "identity" {
                    json!({"user_id":"@other:example.test","device_id":"DEVICE_1"})
                } else {
                    common::who()
                },
            );
            if failure == "room" {
                f.fake.next().await.json(200, common::sync("bootstrap"));
                f.fake.next().await.json(200, json!([]));
            }
        }
        let status = f.wait_result().await;
        assert_eq!(status["state"], "unavailable");
        assert_eq!(status["error"], "refresh");
        assert_eq!(status["workspace_registered"], false);
        assert_eq!(f.attempts(), 0);
        assert!(!f.work.join("owned-mcp.requests").exists());
        drop(child);
    }
}

#[tokio::test]
async fn native_bootstrap_config_disabled() {
    let f = Fixture::new(false).await;
    std::fs::remove_file(f.state_dir.join("development-driver.json")).unwrap();
    let child = f.launch(false);
    let value = f.capabilities().await;
    assert_eq!(value["development_execution"]["state"], "disabled");
    assert_eq!(value["agent_execution"], false);
    assert_eq!(f.attempts(), 0);
    drop(child);
}

#[cfg(unix)]
#[tokio::test]
async fn native_bootstrap_custody_shutdown() {
    let mut f = Fixture::new(false).await;
    let mut child = f.launch(true);
    common::success(&mut f.fake, "shutdown").await;
    // The reception room is observed but never published: answer its /state too.
    f.fake.next().await.json(200, common::state());
    let status = f.wait_result().await;
    assert_eq!(status["protocol"], "completed");
    child.request_shutdown();
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        assert_eq!(status["cleanup"], "unknown");
        let until = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let status = f.capabilities().await["development_execution"].clone();
            if status["error"] == "outcome_unknown" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "unresolved close not reported"
            );
            tokio::task::yield_now().await;
        }
        assert!(child.still_owned());
        assert!(matches!(
            hagency_store::DomainRepository::open(&f.state_dir),
            Err(hagency_store::Error::Locked)
        ));
        assert!(matches!(
            hagency_store::Repository::open(&f.state_dir),
            Err(hagency_store::Error::Locked)
        ));
        assert_eq!(f.attempts(), 1);
        // Test teardown abandons this known-unknown process; it never calls it
        // a clean shutdown or releases its persisted workspace lease.
    } else {
        assert_eq!(status["cleanup"], "whole_tree_stopped");
        child.exited().await;
        drop(hagency_store::DomainRepository::open(&f.state_dir).unwrap());
        drop(hagency_store::Repository::open(&f.state_dir).unwrap());
    }
}
