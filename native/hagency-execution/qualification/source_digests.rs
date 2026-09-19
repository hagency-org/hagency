use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Compile-time source binding, LF-normalized for hosted Windows checkouts.
/// A base commit alone does not identify an uncommitted launch implementation.
pub fn current() -> BTreeMap<String, String> {
    let sources: &[(&str, &[u8])] = &[
        (
            "runtime/owned/session.rs",
            include_bytes!("../../hagency-runtime/src/owned/session.rs"),
        ),
        (
            "runtime/codex/wire.rs",
            include_bytes!("../../hagency-runtime/src/codex/wire.rs"),
        ),
        (
            "runtime/codex/session.rs",
            include_bytes!("../../hagency-runtime/src/codex/session.rs"),
        ),
        (
            "runtime/codex/session/driver.rs",
            include_bytes!("../../hagency-runtime/src/codex/session/driver.rs"),
        ),
        (
            "runtime/codex/session/state.rs",
            include_bytes!("../../hagency-runtime/src/codex/session/state.rs"),
        ),
        (
            "runtime/codex/session/hooks.rs",
            include_bytes!("../../hagency-runtime/src/codex/session/hooks.rs"),
        ),
        (
            "runtime/codex/session/observation.rs",
            include_bytes!("../../hagency-runtime/src/codex/session/observation.rs"),
        ),
        (
            "runtime/codex/approval.rs",
            include_bytes!("../../hagency-runtime/src/codex/approval.rs"),
        ),
        (
            "runtime/codex/session/task_mcp.rs",
            include_bytes!("../../hagency-runtime/src/codex/session/task_mcp.rs"),
        ),
        (
            "platform/unix_spawn.rs",
            include_bytes!("../../hagency-platform/src/unix_spawn.rs"),
        ),
        (
            "platform/supervisor/unix.rs",
            include_bytes!("../../hagency-platform/src/supervisor/unix.rs"),
        ),
        (
            "platform/supervisor/unix/scope.rs",
            include_bytes!("../../hagency-platform/src/supervisor/unix/scope.rs"),
        ),
        (
            "platform/supervisor/unix/macos.rs",
            include_bytes!("../../hagency-platform/src/supervisor/unix/macos.rs"),
        ),
        (
            "platform/supervisor/unix/macos/native.rs",
            include_bytes!("../../hagency-platform/src/supervisor/unix/macos/native.rs"),
        ),
        (
            "platform/supervisor/unix/macos/tracking.rs",
            include_bytes!("../../hagency-platform/src/supervisor/unix/macos/tracking.rs"),
        ),
        (
            "platform/windows.rs",
            include_bytes!("../../hagency-platform/src/windows.rs"),
        ),
        (
            "platform/supervisor/windows.rs",
            include_bytes!("../../hagency-platform/src/supervisor/windows.rs"),
        ),
        (
            "operator/codex_qualify.rs",
            include_bytes!("../examples/codex_qualify.rs"),
        ),
        (
            "operator/codex_qualify/witness.rs",
            include_bytes!("../examples/codex_qualify/witness.rs"),
        ),
        (
            "operator/source_digests.rs",
            include_bytes!("source_digests.rs"),
        ),
    ];
    sources
        .iter()
        .map(|(path, bytes)| {
            let normalized = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
            (
                path.to_string(),
                format!("{:x}", Sha256::digest(normalized.as_bytes())),
            )
        })
        .collect()
}
