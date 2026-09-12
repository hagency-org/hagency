use super::*;
use cap_std::{ambient_authority, fs::Dir};
use hagency_core::{attachments::AttachmentMetadata, tasks::*, uploads::*};
use hagency_media_store::{HostNamespace, OperationId, Store};
use hagency_store::UploadIdentity;
use serde_json::json;

pub(crate) const DATA: &[u8] = b"private original source, not an upload body";
pub(crate) const RAW: &[u8] = b" {\"content_uri\":\"mxc://remote.test/original\"} \n";
pub(crate) struct Fixture {
    pub base: common::Fixture,
    pub collector: Collector,
    pub fake: common::Fake,
    pub cap: RunnerCapability,
    stage: Store,
    workspace: hagency_files::Workspace,
    revoked: bool,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
impl Fixture {
    pub(crate) fn into_file_teardown(self) -> (common::Fixture, Collector, common::Fake) {
        let Self {
            base,
            collector,
            fake,
            ..
        } = self;
        (base, collector, fake)
    }
    pub async fn new() -> Self {
        Self::new_profile(true, false).await
    }
    pub async fn new_profile(direct: bool, thread: bool) -> Self {
        let base = common::Fixture::new();
        let mut fake = common::Fake::start(true).await;
        let mut config = base
            .config(&fake.endpoint)
            .with_root_pem(include_bytes!("../fixtures/ca.pem"))
            .unwrap();
        if !direct {
            config.rooms[0].privacy = hagency_core::replies::RoomPrivacy::Group {};
            config.rooms[0].room_id = "!project:example.test".into();
        }
        let collector = Collector::new(config, base.store.clone()).unwrap();
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(
            collector.collect(&cancel),
            common::success(&mut fake, "upload-initial"),
        )
        .await;
        result.unwrap();
        base.store
            .resolve_verified_matrix_session(SessionBinding {
                id: "upload_session".into(),
                engagement_id: base.identity.transport.engagement_id.clone(),
                room_id: if direct {
                    "!direct:example.test"
                } else {
                    "!project:example.test"
                }
                .into(),
                thread_root: thread.then(|| "$file-root".into()),
            })
            .await
            .unwrap();
        base.store
            .register_workspace("workspace".into())
            .await
            .unwrap();
        base.store
            .create_canonical_task(
                "task".into(),
                "upload_session".into(),
                "Upload fixture".into(),
                now(),
            )
            .await
            .unwrap();
        base.store
            .enqueue_dispatch(DispatchInput {
                id: "upload_dispatch".into(),
                session_id: "upload_session".into(),
                task_id: Some("task".into()),
                resources: vec![ResourceLease {
                    id: "workspace".into(),
                    exclusive: true,
                }],
                payload: json!({"instruction":"fixture"}),
            })
            .await
            .unwrap();
        let cap = base
            .store
            .claim_dispatch("runner".into(), now(), 60_000, 120_000, 8)
            .await
            .unwrap()
            .unwrap();
        base.store.start_dispatch(cap.clone(), now()).await.unwrap();
        let root = base.root.path();
        hagency_store::private::directory(&root.join("stage")).unwrap();
        hagency_store::private::directory(&root.join("work")).unwrap();
        let stage = Store::create(
            Dir::open_ambient_dir(root.join("stage"), ambient_authority()).unwrap(),
            HostNamespace::new("upload-fixture-partition").unwrap(),
            hagency_media_store::Limits::new(4096, 256 * 1024, 32, 8).unwrap(),
        )
        .unwrap();
        let workspace = hagency_files::Workspace::from_directory(
            Dir::open_ambient_dir(root.join("work"), ambient_authority()).unwrap(),
            hagency_files::Limits::new(4096, 8).unwrap(),
        )
        .unwrap();
        Self {
            base,
            collector,
            fake,
            cap,
            stage,
            workspace,
            revoked: false,
        }
    }
    pub async fn input(&mut self, id: &str) -> Option<(StagedUpload, UploadIdentity, Vec<u8>)> {
        let request = UploadRequest {
            call_id: id.into(),
            request_digest: "1".repeat(64),
            metadata: AttachmentMetadata {
                filename: "original.txt".into(),
                mime_type: Some("text/plain".into()),
                declared_size: Some(DATA.len() as u64),
            },
        };
        let admission = self
            .base
            .store
            .reserve_upload(self.cap.clone(), request)
            .await
            .unwrap();
        std::fs::write(self.base.root.path().join("work/source"), DATA).unwrap();
        let snapshot = self
            .workspace
            .snapshot(&hagency_files::RelativeFile::new("source").unwrap())
            .unwrap();
        let codec = hagency_media::Codec::new(hagency_media::Limits::new(4096, 8).unwrap());
        let encrypted = codec.encrypt(snapshot).unwrap();
        let ciphertext = encrypted.ciphertext().to_vec();
        let descriptor = encrypted.descriptor().private_event_json().to_vec();
        let operation = OperationId::new(id).unwrap();
        let prepared = match self.stage.prepare_encrypted(&operation, encrypted) {
            Ok(prepared) => prepared,
            Err(error) => {
                #[cfg(not(windows))]
                panic!("qualified local staging failed: {}", error.error());
                #[cfg(windows)]
                {
                    assert_eq!(error.error(), hagency_media_store::Error::Durability);
                    let Some(hagency_media_store::Media::Encrypted(original)) =
                        error.into_unadmitted()
                    else {
                        panic!("original encryption lost");
                    };
                    assert_eq!(original.ciphertext(), ciphertext);
                    assert_eq!(original.descriptor().private_event_json(), descriptor);
                    assert_eq!(
                        codec
                            .decrypt(original.descriptor(), original.ciphertext())
                            .unwrap()
                            .bytes(),
                        DATA
                    );
                    assert_eq!(
                        self.base
                            .store
                            .inspect_upload(admission.identity)
                            .await
                            .unwrap()
                            .upload,
                        hagency_core::uploads::UploadState::Pending
                    );
                    eprintln!(
                        "ADR089 qualification: FileSyncedDirectoryUnconfirmed; exact encrypted custody returned, POST unavailable"
                    );
                    self.fake.no_request().await;
                    return None;
                }
            }
        };
        let stage = StageCommitment {
            namespace_digest: hex(prepared.namespace().digest()),
            operation_id: id.into(),
            receipt_digest: hex(prepared.digest()),
            len: prepared.len() as u64,
        };
        self.base
            .store
            .bind_upload_stage(
                self.cap.clone(),
                Arc::new(admission.preparation.unwrap()),
                stage.clone(),
            )
            .await
            .unwrap();
        let receipt = self
            .stage
            .stage_prepared(prepared)
            .map_err(|e| e.error())
            .unwrap();
        assert_eq!(hex(receipt.digest()), stage.receipt_digest);
        self.base
            .store
            .observe_upload_staged(
                admission.identity.clone(),
                stage,
                UploadStageObservation::FileAndDirectorySynced,
            )
            .await
            .unwrap();
        let restored = self
            .stage
            .restore_encrypted(&operation, receipt.digest())
            .unwrap();
        assert_eq!(restored.descriptor().private_event_json(), descriptor);
        let claim = self
            .base
            .store
            .claim_upload(self.cap.clone(), admission.identity.clone(), 60_000)
            .await
            .unwrap()
            .unwrap();
        let send = self
            .base
            .store
            .begin_upload(self.cap.clone(), claim.clone())
            .await
            .unwrap();
        let input = StagedUpload::new(self.cap.clone(), claim, send, restored)
            .map_err(|e| e.error())
            .unwrap();
        Some((input, admission.identity, ciphertext))
    }
    pub async fn file_input(
        &mut self,
        id: &str,
        caption: Option<&str>,
    ) -> Option<(StagedUpload, hagency_store::FileDeliveryIdentity, Vec<u8>)> {
        let file = self
            .base
            .store
            .reserve_file_delivery(
                self.cap.clone(),
                hagency_core::file_delivery::FileDeliveryRequest {
                    call_id: id.into(),
                    request_digest: "1".repeat(64),
                    filename: "结果.txt".into(),
                    caption: caption.map(str::to_owned),
                },
            )
            .await
            .unwrap();
        let admission = file.upload;
        std::fs::write(self.base.root.path().join("work/source"), DATA).unwrap();
        let snapshot = self
            .workspace
            .snapshot(&hagency_files::RelativeFile::new("source").unwrap())
            .unwrap();
        let codec = hagency_media::Codec::new(hagency_media::Limits::new(4096, 8).unwrap());
        let encrypted = codec.encrypt(snapshot).unwrap();
        let ciphertext = encrypted.ciphertext().to_vec();
        let descriptor = encrypted.descriptor().private_event_json().to_vec();
        let operation = OperationId::new(id).unwrap();
        let prepared = match self.stage.prepare_encrypted(&operation, encrypted) {
            Ok(prepared) => prepared,
            Err(error) => {
                #[cfg(not(windows))]
                panic!("qualified local staging failed: {}", error.error());
                #[cfg(windows)]
                {
                    assert_eq!(error.error(), hagency_media_store::Error::Durability);
                    let Some(hagency_media_store::Media::Encrypted(original)) =
                        error.into_unadmitted()
                    else {
                        panic!("original encryption lost");
                    };
                    assert_eq!(original.ciphertext(), ciphertext);
                    assert_eq!(original.descriptor().private_event_json(), descriptor);
                    assert_eq!(
                        codec
                            .decrypt(original.descriptor(), original.ciphertext())
                            .unwrap()
                            .bytes(),
                        DATA
                    );
                    assert_eq!(
                        self.base
                            .store
                            .inspect_upload(admission.identity)
                            .await
                            .unwrap()
                            .upload,
                        hagency_core::uploads::UploadState::Pending
                    );
                    eprintln!(
                        "ADR089 qualification: FileSyncedDirectoryUnconfirmed; exact encrypted custody returned, POST unavailable"
                    );
                    self.fake.no_request().await;
                    return None;
                }
            }
        };
        let stage = StageCommitment {
            namespace_digest: hex(prepared.namespace().digest()),
            operation_id: id.into(),
            receipt_digest: hex(prepared.digest()),
            len: prepared.len() as u64,
        };
        self.base
            .store
            .bind_file_delivery_stage(
                self.cap.clone(),
                file.identity.clone(),
                Arc::new(admission.preparation.unwrap()),
                hagency_core::file_delivery::CapturedFile {
                    size: DATA.len() as u64,
                    sha256: crate::outgoing::state::hash(DATA),
                },
                stage.clone(),
            )
            .await
            .unwrap();
        let receipt = self
            .stage
            .stage_prepared(prepared)
            .map_err(|e| e.error())
            .unwrap();
        assert_eq!(hex(receipt.digest()), stage.receipt_digest);
        self.base
            .store
            .observe_upload_staged(
                admission.identity.clone(),
                stage,
                UploadStageObservation::FileAndDirectorySynced,
            )
            .await
            .unwrap();
        let restored = self
            .stage
            .restore_encrypted(&operation, receipt.digest())
            .unwrap();
        assert_eq!(restored.descriptor().private_event_json(), descriptor);
        let claim = self
            .base
            .store
            .claim_upload(self.cap.clone(), admission.identity.clone(), 60_000)
            .await
            .unwrap()
            .unwrap();
        let send = self
            .base
            .store
            .begin_upload(self.cap.clone(), claim.clone())
            .await
            .unwrap();
        let input = StagedUpload::new(self.cap.clone(), claim, send, restored)
            .map_err(|e| e.error())
            .unwrap();
        Some((input, file.identity, ciphertext))
    }
    pub fn admit(&self, input: StagedUpload) -> UploadOperation {
        self.collector
            .stage_upload(input)
            .map_err(|e| e.error())
            .unwrap()
    }
    pub async fn revoke(&mut self) {
        self.base
            .store
            .revoke(
                "revoke-upload".into(),
                self.base.identity.transport.engagement_id.clone(),
            )
            .await
            .unwrap();
        self.revoked = true;
    }
    pub async fn finish(self) {
        // Unknown uploads deliberately retain Collector custody until explicit
        // host teardown. This fixture makes no inference from dropping it.
        let result = self.collector.close().await;
        if self.revoked && result != Err(Error::Busy) {
            // Existing close reports retired domain authority. Preserve that
            // result; close only the remaining private SDK owner explicitly.
            assert_eq!(result, Err(Error::Domain));
            if let Some(owner) = self.collector.inner.owner.lock().await.take() {
                owner.close().await.unwrap();
            }
        } else {
            assert!(
                result.is_ok() || result == Err(Error::Busy),
                "close: {result:?}"
            );
        }
        self.fake.close().await;
        self.base.store.shutdown().await.unwrap();
    }
}
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) async fn authenticate(fake: &mut common::Fake) {
    let request = fake.next().await;
    assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
    assert_eq!(
        request.headers["authorization"],
        format!("Bearer {}", common::TOKEN)
    );
    request.json(200, common::who());
}
pub(crate) async fn post(fake: &mut common::Fake, ciphertext: &[u8]) {
    authenticate(fake).await;
    let request = fake.next().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.target, "/_matrix/media/v3/upload");
    assert_eq!(
        request.headers["authorization"],
        format!("Bearer {}", common::TOKEN)
    );
    assert_eq!(request.headers["content-type"], "application/octet-stream");
    assert_eq!(request.body, ciphertext);
    assert_ne!(request.body, DATA);
    request.raw(common::response(200, RAW));
}

const CHILD: &str = "HAGENCY_STAGED_UPLOAD_SETTLEMENT_FIXTURE";
/// Actual fresh process; no original capability, claim, Send, descriptor or
/// response is passed. This tests host-known selector recovery, not discovery.
pub(crate) async fn child_settlement() -> bool {
    let Some(path) = std::env::var_os(CHILD) else {
        return false;
    };
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() < 4096);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let root = std::path::Path::new(&path).parent().unwrap();
    let identity = crate::HostIdentity {
        server_name: "example.test".into(),
        registration_fingerprint: "a".repeat(64),
        transport: hagency_core::replies::MatrixTransportObservation {
            engagement_id: value["engagement"].as_str().unwrap().into(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        },
    };
    let config = crate::HostConfig::new(
        identity,
        value["origin"].as_str().unwrap(),
        common::TOKEN,
        root.join("sdk"),
        [42; 32],
        vec![crate::HostRoom {
            room_id: "!direct:example.test".into(),
            generation: 1,
            privacy: hagency_core::replies::RoomPrivacy::Direct {
                human_mxid: "@owner:example.test".into(),
            },
        }],
        common::limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("../fixtures/ca.pem"))
    .unwrap();
    let db = hagency_store::DomainRepository::open(&root.join("domain")).unwrap();
    let domain = hagency_store::DomainStore::start(db, 8).unwrap();
    let collector = Collector::new(config, domain.clone()).unwrap();
    let receipt = collector
        .settle_upload(value["id"].as_str().unwrap(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(receipt.upload, hagency_core::uploads::UploadState::Accepted);
    assert_eq!(
        receipt.replayed,
        value["already_accepted"].as_bool().unwrap()
    );
    assert!(
        collector
            .settle_upload(value["id"].as_str().unwrap(), &CancellationToken::new())
            .await
            .unwrap()
            .replayed
    );
    collector.close().await.unwrap();
    drop(collector);
    domain.shutdown().await.unwrap();
    true
}
impl Fixture {
    pub async fn settle_in_new_process(self, id: String, already_accepted: bool) {
        let Self {
            base,
            collector,
            mut fake,
            cap,
            stage,
            workspace,
            revoked,
        } = self;
        assert!(!revoked);
        let closed = collector.close().await;
        if already_accepted {
            closed.unwrap();
        } else {
            assert_eq!(closed, Err(Error::Busy));
            // Simulated full host teardown only after the real private journal
            // proves Accepted; the domain is still WritePossible here.
            let mut owner = collector.inner.owner.lock().await;
            let reference = owner
                .as_ref()
                .unwrap()
                .restore_upload_reference(&id)
                .await
                .unwrap();
            assert_eq!(
                owner
                    .as_ref()
                    .unwrap()
                    .inspect_upload(&reference)
                    .await
                    .unwrap()
                    .phase(),
                Phase::Accepted
            );
            owner.take().unwrap().close().await.unwrap();
        }
        drop(collector);
        base.store.shutdown().await.unwrap();
        let common::Fixture {
            root,
            store,
            identity,
        } = base;
        drop((store, cap, stage, workspace));
        let path = root.path().join("restart-selector.json");
        std::fs::write(&path, serde_json::to_vec(&json!({"id":id,"origin":fake.endpoint,"engagement":identity.transport.engagement_id,"already_accepted":already_accepted})).unwrap()).unwrap();
        let stdout = std::fs::File::create(root.path().join("child.stdout")).unwrap();
        let stderr = std::fs::File::create(root.path().join("child.stderr")).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "upload::tests::native_staged_upload_complete",
                "--nocapture",
            ])
            .env_clear()
            .env(CHILD, &path)
            .stdout(stdout)
            .stderr(stderr);
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        let mut child = command.spawn().unwrap();
        let status = tokio::task::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("historical fixture child timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
        .await
        .unwrap();
        assert!(
            status.success(),
            "historical child: {}",
            std::fs::read_to_string(root.path().join("child.stderr")).unwrap()
        );
        assert!(
            std::fs::read_to_string(root.path().join("child.stdout"))
                .unwrap()
                .contains("1 passed")
        );
        fake.no_request().await;
        fake.close().await;
    }
}
