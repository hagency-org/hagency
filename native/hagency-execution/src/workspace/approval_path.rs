//! Ordinary lexical form of an already-held canonical Root. It never reopens a
//! directory or rewrites a callback; custody stays on the retained handle.
use super::Root;
use crate::Failure;

impl Root {
    pub(crate) fn approval_path(&self) -> Result<String, Failure> {
        ordinary_launch_path(&self.path).ok_or(Failure::Admission)
    }
}

/// The working-directory string an owned runner is launched with and reports
/// back in callbacks (ADR-116 amendment). On Windows a canonical path carries
/// the verbatim `\\?\` prefix, which the untrusted path parser refuses by
/// design; this projects disk and UNC roots into their ordinary form and
/// refuses device namespaces and ambiguous aliases. Elsewhere it is the path
/// itself. Tests compare a runner's reported cwd against this value.
pub fn ordinary_launch_path(path: &std::path::Path) -> Option<String> {
    #[cfg(windows)]
    {
        canonical_windows_path(path)
    }
    #[cfg(not(windows))]
    {
        path.to_str().map(str::to_owned)
    }
}

#[cfg(any(windows, test))]
enum DiskPrefix<'a> {
    Drive(u8),
    Unc(&'a str, &'a str),
}

#[cfg(windows)]
fn canonical_windows_path(path: &std::path::Path) -> Option<String> {
    use std::path::{Component, Prefix};
    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    let prefix = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => DiskPrefix::Drive(drive),
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            DiskPrefix::Unc(server.to_str()?, share.to_str()?)
        }
        // DeviceNS and generic Verbatim names never become disk authority.
        _ => return None,
    };
    if components.next()? != Component::RootDir {
        return None;
    }
    let components = components
        .map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    disk_projection(prefix, &components)
}

#[cfg(any(windows, test))]
fn disk_projection(prefix: DiskPrefix<'_>, components: &[&str]) -> Option<String> {
    let mut result = match prefix {
        DiskPrefix::Drive(drive) if drive.is_ascii_alphabetic() => {
            format!("{}:\\", char::from(drive))
        }
        DiskPrefix::Unc(server, share)
            if ordinary_component(server) && ordinary_component(share) =>
        {
            format!("\\\\{server}\\{share}\\")
        }
        _ => return None,
    };
    if !components.iter().all(|part| ordinary_component(part)) {
        return None;
    }
    result.push_str(&components.join("\\"));
    // The output must already be the ordinary lexical representation, never
    // silently normalize a parent traversal or another namespace into it.
    (hagency_core::execution::PathFlavor::Windows
        .normalize(&result)
        .as_ref()
        == Some(&result))
    .then_some(result)
}

#[cfg(any(windows, test))]
fn ordinary_component(value: &str) -> bool {
    if value.is_empty()
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|ch| ch.is_control() || "\\/:*?\"<>|".contains(ch))
    {
        return false;
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str()) {
        return false;
    }
    let serial = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"));
    !serial.is_some_and(|suffix| {
        ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&suffix)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hagency_core::execution::{HostRequest, PathFlavor};
    use serde_json::json;

    #[test]
    fn native_owned_approval_workspace_projection() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("owned 目录");
        hagency_store::private::directory(&work).unwrap();
        let path = work.canonicalize().unwrap();
        let root = Root::open(path.clone()).unwrap();
        let projection = root.approval_path().unwrap();
        assert_eq!(root.path(), path);
        root.check().unwrap();
        #[cfg(not(windows))]
        assert_eq!(projection, path.to_str().unwrap());
        #[cfg(windows)]
        {
            assert!(
                matches!(path.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_)))
            );
            assert_eq!(
                PathFlavor::Windows.normalize(&projection),
                Some(projection.clone())
            );
            assert!(
                PathFlavor::Windows
                    .normalize(path.to_str().unwrap())
                    .is_none()
            );
            for raw in [
                r"\\.\C:\work",
                r"\\?\GLOBALROOT\Device\HarddiskVolume1\work",
                r"C:work",
                r"\work",
                r"C:\work\..\other",
                r"\\?\C:\work.\file",
            ] {
                assert!(
                    canonical_windows_path(std::path::Path::new(raw)).is_none(),
                    "{raw}"
                );
            }
            assert_eq!(
                canonical_windows_path(std::path::Path::new(r"\\?\UNC\server\share\work")),
                Some(r"\\server\share\work".into())
            );
        }
        assert_eq!(
            disk_projection(DiskPrefix::Drive(b'C'), &["work", "目录"]),
            Some("C:\\work\\目录".into())
        );
        assert_eq!(
            disk_projection(DiskPrefix::Unc("server", "share"), &["work"]),
            Some(r"\\server\share\work".into())
        );
        for part in [
            "",
            ".",
            "..",
            "work.",
            "work ",
            "stream:name",
            "a/b",
            "a\\b",
            "NUL",
            "COM1.txt",
            "LPT²",
            "CONOUT$",
        ] {
            assert!(
                disk_projection(DiskPrefix::Drive(b'C'), &[part]).is_none(),
                "{part}"
            );
        }
        for server in ["?", ".", "..", "", "server "] {
            assert!(disk_projection(DiskPrefix::Unc(server, "share"), &["work"]).is_none());
        }
        assert!(disk_projection(DiskPrefix::Drive(b'1'), &["work"]).is_none());
    }

    #[tokio::test]
    async fn native_owned_approval_workspace_scope() {
        use hagency_core::approvals::{ApprovalRpcId, HostApprovalContext, HostApprovalRequest};
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
        for (workspace, cwd, reusable) in [
            (r"C:\work", r"C:\work", true),
            (r"\\server\share\work", r"\\server\share\work", true),
            (r"C:\work", r"\\?\C:\work", false),
            (r"\\server\share\work", r"\\?\UNC\server\share\work", false),
            (r"C:\work", r"\\.\pipe\request", false),
        ] {
            let params = json!({"threadId":"thread", "turnId":"turn", "itemId":"item", "startedAtMs":1, "command":"echo approved", "cwd":cwd, "availableDecisions":["accept","decline"]});
            let original = params.clone();
            let scope = hagency_core::execution::derive(HostRequest {
                agent_id: "worker",
                workspace,
                task_id: None,
                path_flavor: PathFlavor::Windows,
                may_write: true,
                method: "item/commandExecution/requestApproval",
                params: &params,
            });
            assert_eq!(scope.is_some(), reusable, "{cwd}");
            let temp = tempfile::tempdir().unwrap();
            let (domain, cap) = crate::approval_loss::fixture(temp.path());
            let dispatch = domain.owned_dispatch_scope(cap.clone()).await.unwrap();
            domain
                .start_owned_dispatch(cap.clone(), dispatch.fingerprint().into())
                .await
                .unwrap();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let context = HostApprovalContext {
                id: "owned_context".into(),
                connection_id: "connection".into(),
                thread_id: "thread".into(),
                turn_id: "turn".into(),
                workspace_resource: "work".into(),
                workspace: workspace.into(),
                windows_paths: true,
                environment_id: None,
                may_write: true,
                yolo: false,
            };
            let _owned = domain
                .bind_owned_approval_context(
                    cap.clone(),
                    dispatch.fingerprint().into(),
                    context,
                    Instant::now() + Duration::from_secs(5),
                    now + 5000,
                )
                .await
                .unwrap();
            let summary = domain
                .request_owner_approval(
                    cap,
                    HostApprovalRequest {
                        context_id: "owned_context".into(),
                        upstream_id: ApprovalRpcId::String("rpc".into()),
                        item_id: "item".into(),
                        method: "item/commandExecution/requestApproval".into(),
                        params,
                        expires_at: now + 4000,
                    },
                )
                .await
                .unwrap();
            assert_eq!(summary.reusable_scope, reusable, "{cwd}");
            let card = domain.private_approval(summary.id).await.unwrap();
            assert_eq!(
                card.params, original,
                "native callback remains byte-content bound"
            );
            domain.shutdown().await.unwrap();
        }
    }
}
