use super::*;
#[tokio::test]
async fn native_account_bootstrap() {
    let mut f = Fixture::with_account(false, true).await;
    let mut command = f.command(true);
    command
        .env("PATH", "")
        .env("HOME", "/untrusted-ambient-home")
        .env("CODEX_HOME", "/untrusted-ambient-codex")
        .env("OPENAI_API_KEY", "isolated-test-ambient-key");
    let child = command.spawn().unwrap();
    let child = fixture::Running::from_child(child);
    f.fake.next().await.json(200, common::who());
    f.fake
        .next()
        .await
        .json(200, common::sync("managed-bootstrap"));
    f.fake.next().await.json(200, common::state());
    let status = f.wait_result().await;
    assert_eq!(status["protocol"], "completed", "{status}");
    assert_eq!(f.attempts(), 1);
    let actual: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.work.join("account-observed.json")).unwrap())
            .unwrap();
    assert_eq!(
        actual,
        json!({"marker":"bootstrap-selected","same_home":true,"ambient_key":false})
    );
    drop(child);
}
