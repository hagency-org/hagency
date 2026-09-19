use super::fixture::Fixture;
use hagency_store::private;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn native_receive_restart() {
    let mut f = Fixture::new(true, "normal").await;
    let child = f.launch("receive.original");
    f.drive().await;
    let path = f.receipt("first").unwrap()["structuredContent"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = std::fs::read(f.work.join(&path)).unwrap();
    let inherited: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&private::read_secret(&f.work.join("receive-mcp.context")).unwrap())
            .unwrap();
    assert_eq!(inherited.len(), 4);
    child.stop_and_reap();
    let restarted = f.launch("receive.restarted");
    let held = f.fake.next().await;
    assert_eq!(held.target, "/_matrix/client/v3/account/whoami");
    // Hold this actual refresh response. The newly running HTTP application has
    // no original Started binding; old metadata/context cannot create one.
    f.capabilities().await;
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_hagency"));
    command
        .arg("mcp")
        .env_clear()
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "HAGENCY_RUNNER_API_ADDR",
        "HAGENCY_RUNNER_CAPABILITY",
        "HAGENCY_TASK_ID",
        "HAGENCY_RECEIVE_FILE_TOOLS",
    ] {
        command.env(key, inherited.get(key).unwrap());
    }
    #[cfg(windows)]
    command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    let mut helper = command.spawn().unwrap();
    let mut input = helper.stdin.take().unwrap();
    let mut output = helper.stdout.take().unwrap();
    let mut errors = helper.stderr.take().unwrap();
    for message in [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"historical-receive","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"receive_file","arguments":{"event_id":"$incoming"}}}),
    ] {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(b'\n');
        input.write_all(&bytes).await.unwrap();
    }
    drop(input);
    let status = tokio::time::timeout(Duration::from_secs(5), helper.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    let mut bytes = Vec::new();
    (&mut output)
        .take(16385)
        .read_to_end(&mut bytes)
        .await
        .unwrap();
    assert!(bytes.len() <= 16384);
    let response = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .find(|value| value["id"] == 1)
        .unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert!(!response.to_string().contains(&path));
    let mut stderr = Vec::new();
    (&mut errors)
        .take(1025)
        .read_to_end(&mut stderr)
        .await
        .unwrap();
    assert!(stderr.is_empty());
    assert_eq!(std::fs::read(f.work.join(path)).unwrap(), before);
    assert_eq!(f.count("received_files"), 1);
    assert_eq!(f.gets, 1);
    f.fake.no_request().await;
    restarted.stop_and_reap();
    drop(held);
    f.fake.close().await;
}
