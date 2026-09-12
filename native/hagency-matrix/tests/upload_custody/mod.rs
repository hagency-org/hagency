use super::{Owner, Sdk, upload_custody::Command, upload_state as state};
use crate::{
    CancellationToken, Error, HostConfig, HostIdentity, HostRoom, MediaUploadLimits, MediaUploader,
    UploadAttempt, collector::fixtures as common,
};
use hagency_core::{attachments::AttachmentMetadata, replies::*, tasks::*, uploads::*};
use hagency_store::{DomainRepository, EffectOutcome, UploadIdentity, UploadSend};
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::{Arc, OnceLock},
};

fn serial() -> &'static tokio::sync::Mutex<()> {
    static SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| tokio::sync::Mutex::new(()))
}
struct Domain {
    root: tempfile::TempDir,
    db: DomainRepository,
    cap: RunnerCapability,
    transport: MatrixTransportObservation,
    dispatch: usize,
}
impl Domain {
    fn new() -> Self {
        Self::with_thread(None)
    }
    fn with_thread(thread: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("domain")).unwrap();
        db.register(&common::domain::registration()).unwrap();
        let pool = common::domain::resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let proof = common::domain::proof(&common::domain::request("one", "Worker", &pool, 100));
        let engagement = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture".into(),
            },
        )
        .unwrap();
        let transport = MatrixTransportObservation {
            engagement_id: engagement.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        };
        db.observe_matrix_transport(&transport, 1001).unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: engagement.id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!direct:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                },
                joined: BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
            },
            1002,
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "session".into(),
                engagement_id: engagement.id,
                room_id: "!direct:example.test".into(),
                thread_root: thread.map(str::to_owned),
            },
            1003,
        )
        .unwrap();
        db.register_workspace("workspace").unwrap();
        let cap = Self::dispatch(&mut db, 0);
        Self {
            root,
            db,
            cap,
            transport,
            dispatch: 0,
        }
    }
    fn dispatch(db: &mut DomainRepository, n: usize) -> RunnerCapability {
        let task = format!("task{n}");
        db.create_canonical_task(&task, "session", "Verify", 1004)
            .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: format!("dispatch{n}"),
            session_id: "session".into(),
            task_id: Some(task),
            resources: vec![ResourceLease {
                id: "workspace".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"fixture"}),
        })
        .unwrap();
        let cap = db
            .claim_dispatch("runner", 1005, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
        db.start_dispatch(&cap, 1006).unwrap();
        cap
    }
    fn next_dispatch(&mut self) {
        self.db
            .complete_dispatch(&self.cap, &json!({"fixture":"finished"}), 1020)
            .unwrap();
        self.dispatch += 1;
        self.cap = Self::dispatch(&mut self.db, self.dispatch);
    }
    fn send(&mut self, call: &str) -> (UploadIdentity, StageCommitment, UploadSend) {
        let input = UploadRequest {
            call_id: call.into(),
            request_digest: "1".repeat(64),
            metadata: AttachmentMetadata {
                filename: "result.txt".into(),
                mime_type: Some("text/plain".into()),
                declared_size: Some(12),
            },
        };
        let admission = self.db.reserve_upload(&self.cap, &input, 1010).unwrap();
        // Domain observation fixture only: these tests qualify SDK persistence,
        // not physical staging or this HTTP attempt's association with the row.
        let stage = StageCommitment {
            namespace_digest: "2".repeat(64),
            operation_id: call.into(),
            receipt_digest: "3".repeat(64),
            len: 12,
        };
        self.db
            .bind_upload_stage(
                &self.cap,
                admission.preparation.as_ref().unwrap(),
                &stage,
                1011,
            )
            .unwrap();
        self.db
            .observe_upload_staged(
                &admission.identity,
                &stage,
                UploadStageObservation::FileAndDirectorySynced,
                1012,
            )
            .unwrap();
        let claim = self
            .db
            .claim_upload(&self.cap, &admission.identity, 1013, 60_000)
            .unwrap()
            .unwrap();
        let send = self.db.begin_upload(&self.cap, &claim, 1014).unwrap();
        (admission.identity, stage, send)
    }
    fn config(&self, endpoint: &str) -> HostConfig {
        HostConfig::new(
            HostIdentity {
                server_name: "example.test".into(),
                registration_fingerprint: "a".repeat(64),
                transport: self.transport.clone(),
            },
            endpoint,
            common::TOKEN,
            self.root.path().join("sdk"),
            [42; 32],
            vec![HostRoom {
                room_id: "!direct:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                },
            }],
            common::limits(),
        )
        .unwrap()
        .with_root_pem(include_bytes!("../fixtures/ca.pem"))
        .unwrap()
    }
}
fn media() -> hagency_media::Encrypted {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source"), b"actual bounded source").unwrap();
    let dir =
        cap_std::fs::Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let workspace =
        hagency_files::Workspace::from_directory(dir, hagency_files::Limits::default()).unwrap();
    let snapshot = workspace
        .snapshot(&hagency_files::RelativeFile::new("source").unwrap())
        .unwrap();
    hagency_media::Codec::new(hagency_media::Limits::default())
        .encrypt(snapshot)
        .unwrap()
}
async fn response<'a>(
    encrypted: &'a hagency_media::Encrypted,
    config: &HostConfig,
    fake: &mut common::Fake,
    raw: &[u8],
) -> UploadAttempt<'a> {
    let client = MediaUploader::new(config, MediaUploadLimits::new(1024, 1, 1).unwrap()).unwrap();
    let mut attempt = client.prepare(encrypted).unwrap();
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(attempt.send(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/_matrix/media/v3/upload");
        assert_eq!(request.body, encrypted.ciphertext());
        request.raw(common::response(200, raw));
    })
    .await;
    result.unwrap();
    attempt
}
async fn enable(owner: &Owner, domain: &mut Domain, call: &str) -> state::Reference {
    let (_, _, send) = domain.send(call);
    let reference = owner.upload_reference(&send).unwrap();
    let permit = owner.reserve_upload(send).await.unwrap();
    assert_eq!(
        owner.possible_upload(permit).await.unwrap().phase(),
        state::Phase::WritePossible
    );
    reference
}
async fn fault(owner: &Owner, n: u8) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    owner
        .tx
        .try_send(super::Command::Upload(Command::Fixture(n), tx))
        .unwrap_or_else(|_| panic!("fixture queue"));
    rx.await.unwrap().unwrap();
}
fn receipt(view: state::Inspection) -> UploadAcceptance {
    assert_eq!(view.phase(), state::Phase::Accepted);
    view.receipt().unwrap().clone()
}
fn same_receipt(a: &UploadAcceptance, b: &UploadAcceptance) {
    assert_eq!(a.receipt_id, b.receipt_id);
    assert_eq!(a.receipt_digest, b.receipt_digest);
}
const RAW: &[u8] = br#" { "content_uri" : "mxc:\/\/media.remote\/Abc_123" } "#;

// These commands execute inside the actual owner; mutation fixtures never create
// a sealed UploadResponse and cannot mint a production UploadSend.
pub(super) async fn fixture(sdk: &mut Sdk, n: u8) {
    match n {
        0 => sdk.upload_reply_loss = true,
        1 => {
            sdk.client
                .state_store()
                .remove_custom_value(state::KEY)
                .await
                .unwrap();
        }
        2 => {
            sdk.journal.uploads = None;
            sdk.persist().await.unwrap();
        }
        3 => {
            sdk.client
                .state_store()
                .set_custom_value(state::KEY, b"corrupt ciphertext".to_vec())
                .await
                .unwrap();
        }
        4 => {
            let mut value = serde_json::to_value(sdk.uploads.as_ref().unwrap()).unwrap();
            let record = value["records"]
                .as_object_mut()
                .unwrap()
                .values_mut()
                .next()
                .unwrap();
            record["phase"] = json!("accepted");
            record["response"] = serde_json::Value::Null;
            sdk.client
                .state_store()
                .set_custom_value(state::KEY, sdk.cipher.encrypt_value(&value).unwrap())
                .await
                .unwrap();
        }
        9 | 10 => {
            let mut value = serde_json::to_value(sdk.uploads.as_ref().unwrap()).unwrap();
            let record = value["records"]
                .as_object_mut()
                .unwrap()
                .values_mut()
                .next()
                .unwrap();
            if n == 9 {
                record["identity"]["route"]["session_generation"] = json!(0);
            } else {
                record["identity"]["fence"] = json!(hagency_core::JSON_SAFE_MAX + 1);
            }
            let identity: state::Identity =
                serde_json::from_value(record["identity"].clone()).unwrap();
            record["digest"] = json!(identity.digest().unwrap());
            sdk.client
                .state_store()
                .set_custom_value(state::KEY, sdk.cipher.encrypt_value(&value).unwrap())
                .await
                .unwrap();
        }
        5 => {
            let record = sdk
                .uploads
                .as_ref()
                .unwrap()
                .records
                .values()
                .next()
                .unwrap();
            let response = record.response.as_ref().unwrap();
            assert!(sdk.upload_poisoned);
            assert!(response.custody.is_some());
            assert_eq!(response.body, RAW);
        }
        6 => {
            let record = sdk
                .uploads
                .as_ref()
                .unwrap()
                .records
                .values()
                .next()
                .unwrap();
            let response = record.response.as_ref().unwrap();
            assert_eq!(response.body, RAW);
            assert_eq!(response.mxc, "mxc://media.remote/Abc_123");
            assert_eq!(
                response.body_sha256,
                <[u8; 32]>::from(sha2::Sha256::digest(RAW))
            );
            assert!(response.custody.is_none());
        }
        7 => {
            // Real main-journal growth is logically independent of reserved upload bytes.
            // Grow the actual independent main journal while the owner stays open.
            sdk.journal.pending = Some(json!({"fixture": "x".repeat(64 * 1024)}));
            sdk.persist().await.unwrap();
        }
        8 => {
            sdk.journal.pending = None;
            sdk.persist().await.unwrap();
        }
        _ => panic!("unknown fixture"),
    }
}
use sha2::Digest;

#[tokio::test]
async fn native_matrix_upload_custody_persistence() {
    let _serial = serial().lock().await;
    trait Private<A> {
        fn check() {}
    }
    impl<T: ?Sized> Private<()> for T {}
    impl<T: serde::Serialize> Private<u8> for T {}
    impl<T: serde::de::DeserializeOwned> Private<u16> for T {}
    impl<T: std::fmt::Debug> Private<u32> for T {}
    let _ = <state::LivePermit as Private<_>>::check;
    let _ = <state::Reference as Private<_>>::check;
    let _ = <state::Inspection as Private<_>>::check;
    trait Unique<A> {
        fn check() {}
    }
    impl<T: ?Sized> Unique<()> for T {}
    impl<T: Clone> Unique<u8> for T {}
    let _ = <state::LivePermit as Unique<_>>::check;
    let long_root = format!("${}", "r".repeat(600));
    let mut domain = Domain::with_thread(Some(&long_root));
    let mut fake = common::Fake::start(true).await;
    let config = domain.config(&fake.endpoint);
    let owner = Owner::open(&config).await.unwrap();
    let reference = enable(&owner, &mut domain, "one").await;
    let media = media();
    let attempt = response(&media, &config, &mut fake, RAW).await;
    let accepted = receipt(
        owner
            .accept_upload(&reference, attempt.observed_response().unwrap())
            .await
            .unwrap(),
    );
    same_receipt(
        &accepted,
        &receipt(
            owner
                .accept_upload(&reference, attempt.observed_response().unwrap())
                .await
                .unwrap(),
        ),
    );
    let id = reference.id().to_owned();
    let fence = reference.fence();
    let stage = reference.stage().clone();
    let route = reference.route().clone();
    drop(reference);
    owner.close().await.unwrap();
    let owner = Owner::open_existing(&config).await.unwrap();
    assert!(matches!(
        owner.restore_upload_reference("latest").await,
        Err(Error::Config)
    ));
    assert!(matches!(
        owner
            .restore_upload_reference(&format!("upload_{}", "0".repeat(32)))
            .await,
        Err(Error::OutcomeUnknown)
    ));
    let reference = owner.restore_upload_reference(&id).await.unwrap();
    assert_eq!(reference.fence(), fence);
    assert!(reference.stage() == &stage);
    assert!(reference.route() == &route);
    fault(&owner, 6).await;
    same_receipt(
        &accepted,
        &receipt(owner.inspect_upload(&reference).await.unwrap()),
    );
    owner.close().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_custody_nonrearmable() {
    let _serial = serial().lock().await;
    let mut domain = Domain::new();
    let mut fake = common::Fake::start(true).await;
    let config = domain.config(&fake.endpoint);
    let owner = Owner::open(&config).await.unwrap();
    let (_, _, send) = domain.send("old");
    let reference = owner.upload_reference(&send).unwrap();
    let live = owner.reserve_upload(send).await.unwrap();
    owner.close().await.unwrap();
    let owner = Owner::open_existing(&config).await.unwrap();
    assert!(matches!(
        owner.possible_upload(live).await,
        Err(Error::Conflict)
    ));
    assert_eq!(
        owner.inspect_upload(&reference).await.unwrap().phase(),
        state::Phase::Reserved
    );
    let (_, _, send) = domain.send("lost-reserve");
    let lost_reference = owner.upload_reference(&send).unwrap();
    fault(&owner, 0).await;
    assert!(matches!(
        owner.reserve_upload(send).await,
        Err(Error::OutcomeUnknown)
    ));
    assert_eq!(
        owner.inspect_upload(&lost_reference).await.unwrap().phase(),
        state::Phase::Reserved
    );
    let (_, _, send) = domain.send("lost-possible");
    let lost_reference = owner.upload_reference(&send).unwrap();
    let live = owner.reserve_upload(send).await.unwrap();
    fault(&owner, 0).await;
    assert!(matches!(
        owner.possible_upload(live).await,
        Err(Error::OutcomeUnknown)
    ));
    assert_eq!(
        owner.inspect_upload(&lost_reference).await.unwrap().phase(),
        state::Phase::WritePossible
    );
    let reference = enable(&owner, &mut domain, "lost").await;
    let media = media();
    let attempt = response(&media, &config, &mut fake, RAW).await;
    fault(&owner, 0).await;
    assert!(matches!(
        owner
            .accept_upload(&reference, attempt.observed_response().unwrap())
            .await,
        Err(Error::OutcomeUnknown)
    ));
    let accepted = receipt(owner.inspect_upload(&reference).await.unwrap());
    owner.close().await.unwrap();
    let owner = Owner::open_existing(&config).await.unwrap();
    same_receipt(
        &accepted,
        &receipt(owner.inspect_upload(&reference).await.unwrap()),
    );
    owner.close().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_custody_scope() {
    let _serial = serial().lock().await;
    let mut domain = Domain::new();
    let mut fake = common::Fake::start(true).await;
    let config = domain.config(&fake.endpoint);
    let owner = Owner::open(&config).await.unwrap();
    let (identity, stage, send) = domain.send("scope");
    let reference = owner.upload_reference(&send).unwrap();
    let live = owner.reserve_upload(send).await.unwrap();
    let media = media();
    let attempt = response(&media, &config, &mut fake, RAW).await;
    assert!(matches!(
        owner
            .accept_upload(&reference, attempt.observed_response().unwrap())
            .await,
        Err(Error::OutcomeUnknown)
    ));
    owner.possible_upload(live).await.unwrap();
    for variant in 0..5 {
        let mut changed = (*reference.0).clone();
        match variant {
            0 => changed.fence += 1,
            1 => changed.stage.receipt_digest = "9".repeat(64),
            2 => changed.route.room_generation += 1,
            3 => {
                // Context-generated wrong SDK binding remains historical data only.
                let mut value = serde_json::to_value(&changed).unwrap();
                value["sdk"] = json!("wrong");
                changed = serde_json::from_value(value).unwrap();
            }
            _ => {
                let mut value = serde_json::to_value(&changed).unwrap();
                value["binding"] = json!("e".repeat(64));
                changed = serde_json::from_value(value).unwrap();
            }
        }
        let wrong = state::Reference(Arc::new(changed));
        assert!(matches!(
            owner.inspect_upload(&wrong).await,
            Err(Error::Conflict)
        ));
        assert!(matches!(
            owner
                .accept_upload(&wrong, attempt.observed_response().unwrap())
                .await,
            Err(Error::Conflict)
        ));
    }
    domain.db.cancel_upload(&identity, 1015).unwrap();
    let accepted = receipt(
        owner
            .accept_upload(&reference, attempt.observed_response().unwrap())
            .await
            .unwrap(),
    );
    domain
        .db
        .record_upload_acceptance(&identity, reference.0.fence, &stage, &accepted, 1016)
        .unwrap();
    let status = domain.db.inspect_upload(&identity).unwrap();
    assert!(status.cancel_requested);
    assert_eq!(status.upload, UploadState::Accepted);
    assert!(
        domain
            .db
            .claim_upload(&domain.cap, &identity, 1017, 1000)
            .is_err()
    );
    let changed = response(
        &media,
        &config,
        &mut fake,
        br#"{"content_uri":"mxc://media.remote/Changed"}"#,
    )
    .await;
    assert!(matches!(
        owner
            .accept_upload(&reference, changed.observed_response().unwrap())
            .await,
        Err(Error::Conflict)
    ));
    let changed_spacing = response(
        &media,
        &config,
        &mut fake,
        br#"{"content_uri":"mxc://media.remote/Abc_123"}"#,
    )
    .await;
    assert!(matches!(
        owner
            .accept_upload(&reference, changed_spacing.observed_response().unwrap())
            .await,
        Err(Error::Conflict)
    ));
    owner.close().await.unwrap();
    let mut wrong_config = domain.config("https://different.invalid/");
    assert!(matches!(
        Owner::open_existing(&wrong_config).await,
        Err(Error::Identity)
    ));
    wrong_config = domain.config(&fake.endpoint);
    wrong_config.key = [41; 32];
    assert!(Owner::open_existing(&wrong_config).await.is_err());
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_custody_storage_failure() {
    let _serial = serial().lock().await;
    let mut domain = Domain::new();
    let mut fake = common::Fake::start(true).await;
    let config = domain.config(&fake.endpoint);
    let owner = Owner::open(&config).await.unwrap();
    let reference = enable(&owner, &mut domain, "abort").await;
    let media = media();
    let attempt = response(&media, &config, &mut fake, RAW).await;
    let sql = rusqlite::Connection::open(config.root.join("matrix-sdk-state.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER upload_abort BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture write abort'); END;").unwrap();
    assert!(matches!(
        owner
            .accept_upload(&reference, attempt.observed_response().unwrap())
            .await,
        Err(Error::OutcomeUnknown)
    ));
    fault(&owner, 5).await;
    assert!(matches!(
        owner.inspect_upload(&reference).await,
        Err(Error::OutcomeUnknown)
    ));
    sql.execute_batch("DROP TRIGGER upload_abort;").unwrap();
    drop(sql);
    owner.close().await.unwrap();
    let owner = Owner::open_existing(&config).await.unwrap();
    assert_eq!(
        owner.inspect_upload(&reference).await.unwrap().phase(),
        state::Phase::WritePossible
    );
    // Original HTTP attempt is retained by this test's long-lived host, not by
    // SDK reopen. Re-submit exact sealed historical evidence; never repeat POST.
    owner
        .accept_upload(&reference, attempt.observed_response().unwrap())
        .await
        .unwrap();
    fault(&owner, 6).await;
    owner.close().await.unwrap();
    fake.no_request().await;
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_upload_custody_corruption() {
    let _serial = serial().lock().await;
    for variant in [1, 2, 3, 4, 9, 10] {
        let mut domain = Domain::new();
        let config = domain.config("https://localhost.invalid/");
        let owner = Owner::open(&config).await.unwrap();
        enable(&owner, &mut domain, "corrupt").await;
        fault(&owner, variant).await;
        owner.close().await.unwrap();
        let expected = if variant == 10 {
            Error::Identity
        } else {
            Error::Storage
        };
        assert_eq!(
            Owner::open_existing(&config).await.err(),
            Some(expected),
            "corruption variant {variant}"
        );
    }
    let mut domain = Domain::new();
    let config = domain.config("https://localhost.invalid/");
    let owner = Owner::open(&config).await.unwrap();
    enable(&owner, &mut domain, "key").await;
    owner.close().await.unwrap();
    std::fs::remove_file(config.root.join("journal.key")).unwrap();
    assert!(Owner::open_existing(&config).await.is_err());
    assert!(!config.root.join("journal.key").exists());
}

#[tokio::test]
async fn native_matrix_upload_custody_capacity() {
    const CHILD: &str = "HAGENCY_UPLOAD_CAPACITY_FIXTURE";
    if std::env::var_os(CHILD).as_deref() != Some(std::ffi::OsStr::new("isolated")) {
        // This scenario intentionally occupies every process-wide response slot.
        // A module-local serial lock cannot protect file publication or staged
        // uploads in other modules from that fault injection.
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "sdk::upload_fixture::native_matrix_upload_custody_capacity",
                "--exact",
                "--nocapture",
            ])
            .env(CHILD, "isolated")
            .kill_on_drop(true);
        // The isolated child loads this whole unoptimized test executable
        // before running one scenario; hosted Windows needs well over 30 s.
        // This is a fixture budget, not a runtime deadline.
        let output = tokio::time::timeout(std::time::Duration::from_secs(180), command.output())
            .await
            .expect("isolated capacity scenario deadline")
            .expect("isolated capacity scenario process");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed; 0 failed; 0 ignored"),
            "isolated capacity scenario did not pass: {stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let _serial = serial().lock().await;
    let mut domain = Domain::new();
    let mut fake = common::Fake::start(true).await;
    let config = domain.config(&fake.endpoint);
    let owner = Owner::open(&config).await.unwrap();
    let first = enable(&owner, &mut domain, "slot0").await;
    for n in 1..64 {
        if n % 16 == 0 {
            domain.next_dispatch();
        }
        enable(&owner, &mut domain, &format!("slot{n}")).await;
    }
    domain.next_dispatch();
    let (_, _, send) = domain.send("excess");
    assert!(matches!(
        owner.reserve_upload(send).await,
        Err(Error::Capacity)
    ));
    let media = media();
    let mut maximum = RAW.to_vec();
    maximum.resize(state::MAX_BODY, b' ');
    let attempt = response(&media, &config, &mut fake, &maximum).await;
    let held = state::memory().clone().try_acquire_many_owned(64).unwrap();
    assert!(matches!(
        owner
            .accept_upload(&first, attempt.observed_response().unwrap())
            .await,
        Err(Error::Capacity)
    ));
    owner.close().await.unwrap();
    let owner = Owner::open_existing(&config).await.unwrap();
    assert!(matches!(
        owner
            .accept_upload(&first, attempt.observed_response().unwrap())
            .await,
        Err(Error::Capacity)
    ));
    drop(held);
    fault(&owner, 7).await;
    let accepted = receipt(
        owner
            .accept_upload(&first, attempt.observed_response().unwrap())
            .await
            .unwrap(),
    );
    same_receipt(
        &accepted,
        &receipt(owner.inspect_upload(&first).await.unwrap()),
    );
    fault(&owner, 8).await;
    owner.close().await.unwrap();
    fake.close().await;
}
