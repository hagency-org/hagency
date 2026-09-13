use super::fixture::*;
use salvo::prelude::*;
use serde_json::json;
use std::{
    net::{SocketAddr, TcpListener as StdListener},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

fn built() -> PathBuf {
    std::env::var_os("HAGENCY_NATIVE_CONSOLE_ASSETS").map(PathBuf::from).expect("native console qualification requires HAGENCY_NATIVE_CONSOLE_ASSETS from build:native; not a skipped test")
}
fn address() -> SocketAddr {
    StdListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}
fn node() -> PathBuf {
    std::env::var_os("HAGENCY_BROWSER_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("node"))
}
fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../mockup/scripts/native-console-browser.mjs")
}

#[tokio::test]
async fn native_console_browser() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    let acceptor = TcpListener::new(address).try_bind().await.unwrap();
    let server = Server::new(acceptor);
    let handle = server.handle();
    let serving = tokio::spawn(server.try_serve(f.app.clone().router()));
    let url = hagency::console::client::access(&f.root.path().join("state"), address)
        .await
        .unwrap();
    let mut child = Command::new(node())
        .arg(script())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("actual browser tooling must exist");
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(
            format!(
                "{}\n",
                json!({"base":format!("http://{address}"),"url":url,"engagement":f.engagement})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut created = false;
    tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if line == "CREATE_ENGAGEMENT" {
                assert!(!created);
                let id = f.new_engagement().await;
                input
                    .write_all(format!("{}\n", json!({"engagement":id})).as_bytes())
                    .await
                    .unwrap();
                created = true;
            } else if line == "SCOPED_LINK" {
                // One access ticket is outstanding at a time, so the scoped
                // link is minted only after the browser exchanged the
                // read-only one (the walk that precedes this marker).
                let scoped_url = hagency::console::client::configuration_access(
                    &f.root.path().join("state"),
                    address,
                )
                .await
                .unwrap();
                input
                    .write_all(format!("{}\n", json!({"scopedUrl":scoped_url})).as_bytes())
                    .await
                    .unwrap();
            } else {
                println!("{line}");
            }
        }
        assert!(
            child.wait().await.unwrap().success(),
            "real browser assertions failed"
        );
    })
    .await
    .expect("real browser deadline");
    assert!(
        created,
        "browser must discover an engagement created while it is running"
    );
    handle.stop_graceful(Some(Duration::from_secs(2)));
    serving.await.unwrap().unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_console_executable() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let empty_path = root.path().join("empty-path");
    std::fs::create_dir(&empty_path).unwrap();
    let binary = env!("CARGO_BIN_EXE_hagency");
    let init = Command::new(binary)
        .args(["init", "--state-dir"])
        .arg(&state)
        .env_clear()
        .env("PATH", &empty_path)
        .output()
        .await
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let (db, engagement) = seed(&state);
    drop(db);
    let address = address();
    let mut server = Command::new(binary)
        .args(["serve", "--state-dir"])
        .arg(&state)
        .args(["--listen", &address.to_string(), "--console-assets"])
        .arg(built())
        .env_clear()
        .env("PATH", &empty_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if tokio::net::TcpStream::connect(address).await.is_ok() {
                break;
            }
            assert!(
                server.try_wait().unwrap().is_none(),
                "native console process exited before admission"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("native executable startup");
    let link = Command::new(binary)
        .args(["console-access", "--state-dir"])
        .arg(&state)
        .args(["--listen", &address.to_string()])
        .env_clear()
        .env("PATH", &empty_path)
        .output()
        .await
        .unwrap();
    assert!(
        link.status.success(),
        "native access command must work without Node on PATH"
    );
    let url = String::from_utf8(link.stdout).unwrap().trim().to_owned();
    assert!(!url.contains(TOKEN));
    let mut browser = Command::new(node())
        .arg(script())
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    browser.stdin.take().unwrap().write_all(format!("{}\n", json!({"base":format!("http://{address}"),"url":url,"engagement":engagement,"executable":true})).as_bytes()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(45), browser.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    server.kill().await.unwrap();
    server.wait().await.unwrap();
}

fn resource_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../mockup/scripts/native-console-resources-browser.mjs")
}
#[tokio::test]
async fn native_console_resources_browser() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    let resource = native_resource("private_browser_pool");
    f.domain.put_resource(resource.clone()).await.unwrap();
    // Brief 18: the fixture's usage pool (a bound usage source with real
    // observations) is the MEASURED headroom arm; `resource` itself is the
    // unmeasured arm (a declared ceiling, no engagement, no observation).
    let measured = common::resource("private_usage_pool", "private_usage_seat", 1000);
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    let server = Server::new(TcpListener::new(address).try_bind().await.unwrap());
    let handle = server.handle();
    let serving = tokio::spawn(server.try_serve(f.app.clone().router()));
    let url = hagency::console::client::publication_access(&f.root.path().join("state"), address)
        .await
        .unwrap();
    let mut child = Command::new(node())
        .arg(resource_script())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("actual browser tooling must exist");
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(
            format!(
                "{}\n",
                json!({"base":format!("http://{address}"),"url":url,"resource":resource.id(),"measured":measured.id()})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut created = native_resource("private_created_after_build");
    let lock = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let mut steps = Vec::new();
    tokio::time::timeout(Duration::from_secs(90), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            let answer = match line.as_str() {
                "CREATE_RESOURCE" => {
                    f.domain.put_resource(created.clone()).await.unwrap();
                    json!({"resource":created.id()})
                }
                "EDIT_RESOURCE" => {
                    created.ceiling.as_mut().unwrap().tokens = Some(6000.try_into().unwrap());
                    f.domain.edit_resource(created.clone(), None).await.unwrap();
                    json!({"ok":true})
                }
                "HOLD_STORE" => {
                    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
                    json!({"ok":true})
                }
                "RELEASE_STORE" => {
                    lock.execute_batch("COMMIT").unwrap();
                    json!({"ok":true})
                }
                _ => {
                    println!("{line}");
                    continue;
                }
            };
            steps.push(line);
            input
                .write_all(format!("{answer}\n").as_bytes())
                .await
                .unwrap();
        }
        assert!(
            child.wait().await.unwrap().success(),
            "real resource browser assertions failed"
        );
    })
    .await
    .expect("real resource browser deadline");
    assert_eq!(
        steps,
        [
            "CREATE_RESOURCE",
            "EDIT_RESOURCE",
            "HOLD_STORE",
            "RELEASE_STORE"
        ]
    );
    handle.stop_graceful(Some(Duration::from_secs(2)));
    serving.await.unwrap().unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_console_resources_executable() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let empty_path = root.path().join("empty-path");
    std::fs::create_dir(&empty_path).unwrap();
    let binary = env!("CARGO_BIN_EXE_hagency");
    let init = Command::new(binary)
        .args(["init", "--state-dir"])
        .arg(&state)
        .env_clear()
        .env("PATH", &empty_path)
        .output()
        .await
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let (mut db, _) = seed(&state);
    let resource = native_resource("private_executable_resource");
    db.put_resource(&resource).unwrap();
    drop(db);
    // Brief 21: the executable lane seeds the SAME measured pool the plain
    // lane does (seed() plants private_usage_pool with a bound usage source
    // and real observations), so both lanes prove the same headroom cells —
    // the orchestrator's failing run was the driver selecting an id this
    // config never carried. The id is the deterministic resource hash, so
    // constructing the resource computes the seeded row's id.
    let measured = native_resource("private_usage_pool");
    let address = address();
    let mut server = Command::new(binary)
        .args(["serve", "--state-dir"])
        .arg(&state)
        .args(["--listen", &address.to_string(), "--console-assets"])
        .arg(built())
        .env_clear()
        .env("PATH", &empty_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if tokio::net::TcpStream::connect(address).await.is_ok() {
                break;
            }
            assert!(
                server.try_wait().unwrap().is_none(),
                "native console process exited before admission"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("native executable startup");
    let link = Command::new(binary)
        .args([
            "console-access",
            "--manage-resource-publication",
            "--state-dir",
        ])
        .arg(&state)
        .args(["--listen", &address.to_string()])
        .env_clear()
        .env("PATH", &empty_path)
        .output()
        .await
        .unwrap();
    assert!(
        link.status.success(),
        "native access command must work without Node on PATH"
    );
    let url = String::from_utf8(link.stdout).unwrap().trim().to_owned();
    assert!(!url.contains(TOKEN));
    let mut browser = Command::new(node())
        .arg(resource_script())
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    browser.stdin.take().unwrap().write_all(format!("{}\n", json!({"base":format!("http://{address}"),"url":url,"resource":resource.id(),"measured":measured.id(),"executable":true})).as_bytes()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(45), browser.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    server.kill().await.unwrap();
    server.wait().await.unwrap();
}

fn configuration_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../mockup/scripts/native-console-resource-configuration-browser.mjs")
}
#[tokio::test]
async fn native_console_resource_configuration_browser() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    let resource = configuration_resource();
    f.domain.put_resource(resource.clone()).await.unwrap();
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    let server = Server::new(TcpListener::new(address).try_bind().await.unwrap());
    let handle = server.handle();
    let serving = tokio::spawn(server.try_serve(f.app.clone().router()));
    let url = hagency::console::client::configuration_access(&f.root.path().join("state"), address)
        .await
        .unwrap();
    let mut child = Command::new(node())
        .arg(configuration_script())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("actual browser tooling must exist");
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(
            format!(
                "{}\n",
                json!({"base":format!("http://{address}"),"url":url,"resource":resource.id()})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let lock = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let mut created = None;
    let mut unknown = None;
    let mut steps = Vec::new();
    tokio::time::timeout(Duration::from_secs(90), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(id) = line.strip_prefix("CREATED ") {
                let actual = f.domain.resource_configuration(id.into()).await.unwrap();
                assert_eq!(actual.seat_id, resource.seat_id);
                assert_ne!(actual.preset_id, resource.preset_id);
                assert!(actual.published);
                created = Some(id.to_owned());
                continue;
            }
            if let Some(id) = line.strip_prefix("EDITED ") {
                assert_eq!(created.as_deref(), Some(id));
                assert_eq!(
                    u64::from(
                        f.domain
                            .resource_budget(id.into())
                            .await
                            .unwrap()
                            .pool
                            .ceiling
                            .unwrap()
                    ),
                    22222
                );
                continue;
            }
            if let Some(id) = line.strip_prefix("UNKNOWN_CREATED ") {
                let actual = f.domain.resource_configuration(id.into()).await.unwrap();
                assert_eq!(actual.seat_id, resource.seat_id);
                unknown = Some(id.to_owned());
                continue;
            }
            let answer = if let Some(id) = line.strip_prefix("EDIT_RESOURCE ") {
                let mut actual = f.domain.resource_configuration(id.into()).await.unwrap();
                actual.ceiling.as_mut().unwrap().tokens = Some(6000.try_into().unwrap());
                f.domain.edit_resource(actual, None).await.unwrap();
                json!({"ok":true})
            } else {
                match line.as_str() {
                    "HOLD_STORE" => {
                        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
                        json!({"ok":true})
                    }
                    "RELEASE_STORE" => {
                        lock.execute_batch("COMMIT").unwrap();
                        json!({"ok":true})
                    }
                    _ => {
                        println!("{line}");
                        continue;
                    }
                }
            };
            steps.push(line);
            input
                .write_all(format!("{answer}\n").as_bytes())
                .await
                .unwrap();
        }
        assert!(
            child.wait().await.unwrap().success(),
            "real configuration browser assertions failed"
        );
    })
    .await
    .expect("real configuration browser deadline");
    assert!(created.is_some());
    assert!(unknown.is_some());
    assert_eq!(steps.len(), 3);
    assert_eq!(
        f.domain
            .resource_configurations(String::new(), 100)
            .await
            .unwrap()
            .iter()
            .filter(|r| r.config.seat_id == resource.seat_id)
            .count(),
        3
    );
    handle.stop_graceful(Some(Duration::from_secs(2)));
    serving.await.unwrap().unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_console_resource_configuration_executable() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let empty_path = root.path().join("empty-path");
    std::fs::create_dir(&empty_path).unwrap();
    let binary = env!("CARGO_BIN_EXE_hagency");
    let init = Command::new(binary)
        .args(["init", "--state-dir"])
        .arg(&state)
        .env_clear()
        .env("PATH", &empty_path)
        .output()
        .await
        .unwrap();
    assert!(init.status.success());
    let (mut db, _) = seed(&state);
    let source = configuration_resource();
    db.put_resource(&source).unwrap();
    drop(db);
    let mut selected = source.id();
    for restart in [false, true] {
        let address = address();
        let mut server = Command::new(binary)
            .args(["serve", "--state-dir"])
            .arg(&state)
            .args(["--listen", &address.to_string(), "--console-assets"])
            .arg(built())
            .env_clear()
            .env("PATH", &empty_path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if tokio::net::TcpStream::connect(address).await.is_ok() {
                    break;
                }
                assert!(server.try_wait().unwrap().is_none());
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let link = Command::new(binary)
            .args([
                "console-access",
                "--manage-resource-configuration",
                "--state-dir",
            ])
            .arg(&state)
            .args(["--listen", &address.to_string()])
            .env_clear()
            .env("PATH", &empty_path)
            .output()
            .await
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let url = String::from_utf8(link.stdout).unwrap().trim().to_owned();
        assert!(!url.contains(TOKEN));
        let mut browser = Command::new(node())
            .arg(configuration_script())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        browser.stdin.take().unwrap().write_all(format!("{}\n",json!({"base":format!("http://{address}"),"url":url,"resource":selected,"executable":true,"verifyOnly":restart})).as_bytes()).await.unwrap();
        let mut lines = BufReader::new(browser.stdout.take().unwrap()).lines();
        let mut observed = false;
        tokio::time::timeout(Duration::from_secs(60), async {
            while let Some(line) = lines.next_line().await.unwrap() {
                if let Some(id) = line.strip_prefix("CREATED ") {
                    assert!(!restart);
                    selected = id.into();
                    observed = true;
                }
                if let Some(id) = line.strip_prefix("VERIFIED ") {
                    assert!(restart);
                    assert_eq!(id, selected);
                    observed = true;
                }
                println!("{line}");
            }
            assert!(browser.wait().await.unwrap().success());
        })
        .await
        .expect("native configuration executable browser deadline");
        assert!(observed);
        server.kill().await.unwrap();
        server.wait().await.unwrap();
        let db = hagency_store::DomainRepository::open(&state).unwrap();
        let actual = db.resource_configuration(&selected).unwrap();
        assert_eq!(actual.seat_id, source.seat_id);
        assert_eq!(u64::from(actual.ceiling.unwrap().tokens.unwrap()), 22222);
        drop(db);
    }
}

fn accounts_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../mockup/scripts/native-console-accounts-browser.mjs")
}
#[tokio::test]
async fn native_console_accounts_browser() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    // Seed one enrolled account through the store's own wrappers, then read
    // its identity values straight from the row the page must not reveal.
    let reserved = f
        .domain
        .reserve_account(hagency_store::ACCOUNT_PROFILE.to_owned())
        .await
        .unwrap();
    let choice = f
        .domain
        .materialize_account(reserved.id.clone())
        .await
        .unwrap();
    let managed = f.domain.managed_account(choice.id.clone()).await.unwrap();
    let access = hagency_store::AccountEnrollmentAccess::new(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        Default::default(),
    );
    let command = access
        .prepare(
            &managed,
            choice.revision.clone(),
            "gpt-5.6-sol".into(),
            Some("medium".into()),
            None,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .unwrap();
    f.domain.enroll_account_resource(command).await.unwrap();
    let secrets = {
        let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
        sql.query_row(
            "SELECT a.seat_id,a.identity_tuple,json_extract(a.namespace_identity,'$.volume'),r.preset_id FROM managed_accounts a JOIN resource_accounts r ON r.account_id=a.id LIMIT 1",
            [],
            |row| {
                Ok([
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ])
            },
        )
        .unwrap()
    };
    let server = Server::new(TcpListener::new(address).try_bind().await.unwrap());
    let handle = server.handle();
    let serving = tokio::spawn(server.try_serve(f.app.clone().router()));
    let url = hagency::console::client::account_access(&f.root.path().join("state"), address)
        .await
        .unwrap();
    assert!(url.contains("/console/accounts/#access="));
    let mut child = Command::new(node())
        .arg(accounts_script())
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("actual browser tooling must exist");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            format!(
                "{}\n",
                json!({"base":format!("http://{address}"),"url":url,"account":choice.id,"secrets":secrets})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(45), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success(),
        "real accounts browser assertions failed"
    );
    handle.stop_graceful(Some(Duration::from_secs(2)));
    serving.await.unwrap().unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_console_agent_roster_browser() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    let acceptor = TcpListener::new(address).try_bind().await.unwrap();
    let server = Server::new(acceptor);
    let handle = server.handle();
    let serving = tokio::spawn(server.try_serve(f.app.clone().router()));
    // The harness owns the ticket — the driver receives only a URL, and it
    // mints nothing itself; one outstanding ticket at a time.
    let url = hagency::console::client::access(&f.root.path().join("state"), address)
        .await
        .unwrap();
    let mut child = Command::new(node())
        .arg(script())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("actual browser tooling must exist");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            format!(
                "{}\n",
                json!({"base":format!("http://{address}"),"url":url,"roster":true})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let output = child.wait_with_output().await.unwrap();
    assert!(
        output.status.success(),
        "real roster browser assertions failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PASS native agent roster browser"),
        "the roster lane reported no pass marker"
    );
    handle.stop_graceful(Some(Duration::from_secs(2)));
    serving.await.unwrap().unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_console_project_side_browser() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    let acceptor = TcpListener::new(address).try_bind().await.unwrap();
    let server = Server::new(acceptor);
    let handle = server.handle();
    let serving = tokio::spawn(server.try_serve(f.app.clone().router()));
    // The harness owns the ticket — the driver receives only a URL and
    // mints nothing itself; one outstanding ticket at a time.
    let url = hagency::console::client::access(&f.root.path().join("state"), address)
        .await
        .unwrap();
    let mut child = Command::new(node())
        .arg(script())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("actual browser tooling must exist");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            format!(
                "{}\n",
                json!({"base":format!("http://{address}"),"url":url,"sides":true})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let output = child.wait_with_output().await.unwrap();
    assert!(
        output.status.success(),
        "real project-sides browser assertions failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PASS native project-sides browser"),
        "the project-sides lane reported no pass marker"
    );
    handle.stop_graceful(Some(Duration::from_secs(2)));
    serving.await.unwrap().unwrap();
    f.close().await;
}
