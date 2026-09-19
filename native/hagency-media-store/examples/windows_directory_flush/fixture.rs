use super::{Failure, Result, handles, token::Ordinary};
use cap_std::fs::Dir;
use hagency_files::{RelativeFile, Workspace};
use hagency_media::Codec;
use hagency_media_store::{HostNamespace, Limits, OperationId, Recovery, Store, SyncEvidence};
use hagency_store::private;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

const CONTENT: &[u8] = b"original private media fixture\0\xff";
const OPERATION: &str = "original";
const NAMESPACE: &str = "directory-probe-only";

struct OwnedChild(Option<Child>);
impl OwnedChild {
    fn wait(mut self) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let child = self.0.as_mut().ok_or(Failure::refused("child_owner"))?;
            if let Some(status) = child.try_wait().map_err(|e| Failure::io("child_wait", e))? {
                self.0.take(); // try_wait already reaped the exact child.
                return if status.success() {
                    Ok(())
                } else {
                    Err(Failure {
                        phase: "original_child_verdict",
                        code: i64::from(status.code().unwrap_or(-1)),
                    })
                };
            }
            if Instant::now() >= until {
                return Err(Failure::refused("child_timeout_unknown"));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            // Keep the original child owner through a bounded reap observation.
            // Kernel termination is not claimed when this acknowledgement fails.
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            eprintln!("{{\"phase\":\"child_reap_unknown\",\"qualified\":false,\"code\":-1}}");
            // The controller exits; it must not continue with another child or
            // recursively clean up storage still potentially owned by this one.
            std::process::exit(78);
        }
    }
}
fn launch(root: &Path, mode: &'static str) -> Result<()> {
    let mut command =
        Command::new(std::env::current_exe().map_err(|e| Failure::io("executable", e))?);
    command
        .current_dir(root)
        .env("HAGENCY_DIRECTORY_PROBE_CHILD", mode);
    #[cfg(test)]
    command.args(["--exact", "native_windows_directory_probe", "--nocapture"]);
    let child = command.spawn().map_err(|e| Failure::io("child_start", e))?;
    OwnedChild(Some(child)).wait()
}
pub(super) fn controller() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("hagency-directory-probe-")
        .tempdir()
        .map_err(|e| Failure::io("fixture_create", e))?;
    // Preserve failed fixture evidence and never recursively remove replacements.
    let ancestor = temporary.keep();
    let root = ancestor.join("case");
    private::directory(&root).map_err(|_| Failure::refused("fixture_private"))?;
    // Children inherit no tested filesystem handles. They create/open tested
    // objects only after effective ordinary-token verification.
    launch(&root, "stage")?;
    launch(&root, "restore")?;
    // Both original processes have exited; no recursive replacement cleanup.
    for name in ["input.bin", "receipt.bin"] {
        std::fs::remove_file(root.join(name))
            .map_err(|e| Failure::io("fixture_file_cleanup", e))?;
    }
    std::fs::remove_file(root.join("stage/media.journal"))
        .map_err(|e| Failure::io("fixture_journal_cleanup", e))?;
    std::fs::remove_dir(root.join("stage")).map_err(|e| Failure::io("fixture_stage_cleanup", e))?;
    std::fs::remove_dir(&root).map_err(|e| Failure::io("fixture_root_cleanup", e))?;
    std::fs::remove_dir(ancestor).map_err(|e| Failure::io("fixture_cleanup", e))?;
    println!(
        "{{\"phase\":\"qualification\",\"local_ntfs_os_ack\":true,\"fresh_process_restore\":true,\"default_store\":true,\"production_enabled\":false}}"
    );
    Ok(())
}
pub(super) fn child(restore: bool) -> Result<()> {
    let _ordinary = Ordinary::enter()?;
    let root = std::env::current_dir().map_err(|e| Failure::io("fixture_cwd", e))?;
    if root.file_name().is_none_or(|name| name != "case") {
        return Err(Failure::refused("fixture_name"));
    }
    let root_dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority())
        .map_err(|e| Failure::io("root_open", e))?;
    private::check_handle(
        &root_dir
            .try_clone()
            .map_err(|e| Failure::io("root_clone", e))?
            .into_std_file(),
    )
    .map_err(|_| Failure::refused("root_private"))?;
    let path = root.join("stage");
    if !restore {
        if path.exists() {
            return Err(Failure::refused("fresh_stage"));
        }
        private::directory(&path).map_err(|_| Failure::refused("stage_private"))?;
    }
    // Ambient open is the initial host provisioning step; the sync candidate
    // itself is derived only from this retained capability via relative dot.
    let original = root_dir
        .open_dir("stage")
        .map_err(|e| Failure::io("stage_open", e))?;
    let candidate = handles::candidate(&original)?;
    let original_id = handles::identity(&candidate)?;
    if !restore {
        handles::mutate(&original, &candidate)?;
    }
    handles::held_rename(&root_dir)?;
    println!("{{\"phase\":\"held_rename\",\"sharing_violation\":32}}");
    let namespace = HostNamespace::new(NAMESPACE).map_err(|_| Failure::refused("namespace"))?;
    let operation = OperationId::new(OPERATION).map_err(|_| Failure::refused("operation"))?;
    let limits = Limits::new(4096, 65536, 1, 1).map_err(|_| Failure::refused("limits"))?;
    // ADR104 qualifies the default production opening path. The independent
    // candidate is diagnostics only and is never injected into the Store.
    drop(candidate);
    let directory = original;
    let mut store = if restore {
        Store::open(directory, namespace, limits)
    } else {
        Store::create(directory, namespace, limits)
    }
    .map_err(|_| Failure::refused("store_open"))?;
    if store.sync_evidence() != SyncEvidence::FileAndDirectorySynced
        || store.recovery() != Recovery::Clean
    {
        return Err(Failure::refused("store_real_sync"));
    }
    let codec = Codec::new(
        hagency_media::Limits::new(4096, 1).map_err(|_| Failure::refused("codec_limits"))?,
    );
    if restore {
        let mut receipt = Vec::with_capacity(97);
        private::open(&root.join("receipt.bin"), false)
            .map_err(|_| Failure::refused("receipt_open"))?
            .take(97)
            .read_to_end(&mut receipt)
            .map_err(|e| Failure::io("receipt_read", e))?;
        if receipt.len() != 96 {
            return Err(Failure::refused("receipt_bound"));
        }
        let digest: [u8; 32] = receipt[..32]
            .try_into()
            .map_err(|_| Failure::refused("receipt_digest"))?;
        let retained = store
            .restore_encrypted(&operation, &digest)
            .map_err(|_| Failure::refused("restore_original"))?;
        if Sha256::digest(retained.ciphertext())[..] != receipt[32..64]
            || Sha256::digest(retained.descriptor().private_event_json())[..] != receipt[64..96]
        {
            return Err(Failure::refused("restored_commitment"));
        }
        let checked = codec
            .decrypt(retained.descriptor(), retained.ciphertext())
            .map_err(|_| Failure::refused("decrypt_original"))?;
        if checked.bytes() != CONTENT {
            return Err(Failure::refused("original_plaintext"));
        }
        println!("{{\"phase\":\"fresh_process_restore\",\"original_commitments\":true}}");
    } else {
        private::write_new(&root.join("input.bin"), CONTENT)
            .map_err(|_| Failure::refused("source_create"))?;
        let workspace = Workspace::from_directory(
            root_dir
                .try_clone()
                .map_err(|e| Failure::io("workspace_clone", e))?,
            hagency_files::Limits::new(4096, 1)
                .map_err(|_| Failure::refused("workspace_limits"))?,
        )
        .map_err(|_| Failure::refused("workspace"))?;
        let snapshot = workspace
            .snapshot(&RelativeFile::new("input.bin").map_err(|_| Failure::refused("selection"))?)
            .map_err(|_| Failure::refused("snapshot"))?;
        let encrypted = codec
            .encrypt(snapshot)
            .map_err(|_| Failure::refused("encrypt"))?;
        let ciphertext: [u8; 32] = Sha256::digest(encrypted.ciphertext()).into();
        let descriptor: [u8; 32] =
            Sha256::digest(encrypted.descriptor().private_event_json()).into();
        let prepared = store
            .prepare_encrypted(&operation, encrypted)
            .map_err(|_| Failure::refused("prepare"))?;
        let digest = *prepared.digest();
        let mut commitment = [0u8; 96];
        commitment[..32].copy_from_slice(&digest);
        commitment[32..64].copy_from_slice(&ciphertext);
        commitment[64..].copy_from_slice(&descriptor);
        private::write_new(&root.join("receipt.bin"), &commitment)
            .map_err(|_| Failure::refused("original_receipt"))?;
        let receipt = store
            .stage_prepared(prepared)
            .map_err(|_| Failure::refused("stage_original"))?;
        if receipt.digest() != &digest
            || receipt.sync_evidence() != SyncEvidence::FileAndDirectorySynced
        {
            return Err(Failure::refused("staged_ack"));
        }
        println!("{{\"phase\":\"stage_original\",\"qualified_receipt\":true}}");
    }
    drop(store);
    handles::rename_released(&root_dir, false, original_id)?;
    handles::rename_released(&root_dir, true, original_id)?;
    println!("{{\"phase\":\"released_rename\",\"ack\":true}}");
    Ok(())
}
