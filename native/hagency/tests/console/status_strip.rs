//! ADR-145: the console readiness and version strip — the three spec
//! scenarios. Two drive a real browser against a real console fixture
//! (the 503 boundary and the unreachable read); the third holds the
//! built bundle to the workspace's own version, asserted on the value,
//! never a chunk hash or size. Every selector sits behind the
//! native-console-browser feature exactly as the usage console spec
//! binds it: the scenarios need the qualified asset bundle and a
//! browser, which only the browser lane provides.

use super::fixture::*;
use salvo::prelude::*;
use serde_json::json;
use std::{
    net::{SocketAddr, TcpListener as StdListener},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

fn built() -> PathBuf {
    std::env::var_os("HAGENCY_NATIVE_CONSOLE_ASSETS")
        .map(PathBuf::from)
        .expect("native console qualification requires HAGENCY_NATIVE_CONSOLE_ASSETS from build:native; not a skipped test")
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

/// The scenario driver, written to a tempdir at run time: the strip's
/// assertions belong to the test, not to a committed script (the spec's
/// Allowed Changes names no driver path). stdio contract: one JSON
/// config line in, one PASS line out — the sibling lane's shape.
const DRIVER: &str = r#"/* Status-strip qualification (ADR-145). No repo file: the test writes
 * this driver into its own tempdir; NODE_PATH resolves playwright-core
 * from the workspace's mockup/node_modules. */
const assert = require('node:assert/strict');
const { createInterface } = require('node:readline');
const { chromium } = require('playwright-core');
(async () => {
  const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
  const config = JSON.parse((await lines.next()).value);
  const browser = await chromium.launch({ executablePath: process.env.HAGENCY_BROWSER_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true,
    args: ['--disable-background-networking', '--disable-component-update', '--no-default-browser-check'] });
  try {
    const context = await browser.newContext({ serviceWorkers: 'block' });
    const page = await context.newPage();
    const failures = [];
    page.on('pageerror', (error) => failures.push(error.message));
    if (config.scenario === 'unreachable') await context.route('**/ready', (route) => route.abort('failed'));
    await page.goto(config.url);
    const expected = config.scenario === 'unreachable' ? 'unknown' : 'not-ready';
    await page.locator('[data-native-status="' + expected + '"]').waitFor();
    const word = (await page.locator('[data-native-status-cell="readiness"]').innerText()).trim();
    const version = (await page.locator('[data-native-status-cell="version"]').innerText()).trim();
    if (config.scenario === 'unreachable') {
      assert.match(word, /unknown/i);
      assert.equal(await page.locator('[data-native-status-cell="components"]').count(), 0);
    } else {
      assert.match(word, /not ready/i);
      const components = await page.locator('[data-native-status-cell="components"]').innerText();
      assert.match(components, /domain_writer=closed/);
    }
    assert(!/:\s*ready$/i.test(word), 'the strip must never render the ready word here');
    assert.match(version, /:\s*\S+/, 'the version cell renders the constant');
    assert.deepEqual(failures, []);
    console.log('PASS status strip ' + config.scenario);
  } finally { await browser.close(); }
  process.exit(0);
})().catch((error) => { console.error(error); process.exit(1); });
"#;

/// Serve the app, mint the read-only console URL, and run the driver
/// against it — the sibling browser lane's shape minus its interactive
/// engagement protocol, which the strip does not exercise.
async fn drive(app: hagency::App, root: &Path, address: SocketAddr, scenario: &str) {
    let acceptor = TcpListener::new(address).try_bind().await.unwrap();
    let server = Server::new(acceptor);
    let handle = server.handle();
    let serving = tokio::spawn(server.try_serve(app.router()));
    let url = hagency::console::client::access(&root.join("state"), address)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("status-strip-driver.cjs");
    std::fs::write(&script, DRIVER).unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new(node())
        .arg(&script)
        .env("NODE_PATH", manifest.join("../../mockup/node_modules"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            format!(
                "{}\n",
                json!({"base": format!("http://{address}"), "url": url, "scenario": scenario})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    drop(child.stdin.take());
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut passed = false;
    while let Some(line) = lines.next_line().await.unwrap() {
        println!("{line}");
        passed |= line.starts_with("PASS status strip");
    }
    assert!(passed, "the status-strip driver did not report PASS");
    assert!(child.wait().await.unwrap().success(), "driver failed");
    handle.stop_graceful(Some(Duration::from_secs(2)));
    serving.await.unwrap().unwrap();
}

/// The 503 boundary: a closed domain writer drives `/ready` to 503 while
/// the console keeps serving pages — the strip's whole case. The operator
/// sees not ready with the failing component's own name and state word,
/// and never the word ready.
#[cfg(feature = "native-console-browser")]
#[tokio::test]
async fn native_console_status_strip_503_renders_not_ready() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    f.domain.shutdown().await.unwrap();
    drive(f.app.clone(), f.root.path(), address, "unavailable").await;
    // The domain writer is already drained; close only what is still open.
    f.console.retire();
    f.custody.shutdown().await.unwrap();
}

/// The unreachable read: the driver aborts `/ready` at the network level
/// against an otherwise healthy fixture, so the unknown word can only
/// come from the failed fetch — and no component list renders.
#[cfg(feature = "native-console-browser")]
#[tokio::test]
async fn native_console_status_strip_unreachable_renders_unknown() {
    let address = address();
    let f = Fixture::new(address, Some(&built()));
    hagency_store::private::write_new(
        &f.root.path().join("state/operator.token"),
        TOKEN.as_bytes(),
    )
    .unwrap();
    drive(f.app.clone(), f.root.path(), address, "unreachable").await;
    f.close().await;
}

/// The built assets carry the workspace version after a rebuild: the
/// real bundle located through `HAGENCY_NATIVE_CONSOLE_ASSETS`, never
/// the synthetic console fixture. A missing bundle (the `expect` above)
/// or a missing constant fails the test — it never skips. The assertion
/// is on the bundled value, never a chunk hash or size.
#[cfg(feature = "native-console-browser")]
#[test]
fn native_console_status_strip_version_matches_workspace() {
    let assets = built();
    let toml =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .unwrap();
    let table = toml
        .split_once("[workspace.package]")
        .expect("the root Cargo.toml carries [workspace.package]")
        .1;
    let mut expected = None;
    for line in table.lines() {
        if line.starts_with('[') {
            break;
        }
        let Some(rest) = line.trim_start().strip_prefix("version") else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            expected = Some(value.to_owned());
        }
    }
    let expected = expected.expect("a version line inside [workspace.package]");
    fn scripts(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                scripts(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "js") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    scripts(&assets, &mut files);
    assert!(!files.is_empty(), "the asset bundle carries no scripts");
    const MARKER: &str = "__hagencyNativeVersion";
    let mut bundled = None;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let Some(at) = text.find(MARKER) else {
            continue;
        };
        let rest = &text[at + MARKER.len()..];
        let open = rest.find('"').expect("the version is a quoted literal");
        let close = rest[open + 1..]
            .find('"')
            .expect("the version literal closes");
        bundled = Some(rest[open + 1..open + 1 + close].to_owned());
    }
    let bundled = bundled.expect("the built bundle carries HAGENCY_NATIVE_VERSION");
    assert_eq!(
        bundled, expected,
        "the bundled version must equal the workspace's own"
    );
}
