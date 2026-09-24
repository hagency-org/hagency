use super::{DriverMode, Failure};
use hagency_core::replies::{MatrixTransportObservation, RoomPrivacy};
use hagency_execution::{Host, Limits};
use hagency_matrix::{HostConfig, HostIdentity, HostRoom};
use hagency_store::{DomainRepository, OwnedClaimProfile, OwnedClaimRoom, private};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
};

const CONFIG_BYTES: usize = 16 * 1024;
const EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    profile: String,
    #[serde(default)]
    managed_account: Option<String>,
    #[serde(default)]
    local_codex: Option<LocalCodex>,
    #[serde(default)]
    send_file: bool,
    #[serde(default)]
    receive_file: bool,
    /// ADR180: let owned Codex dispatches call the coordination tools.
    #[serde(default)]
    coordination_tools: bool,
    #[serde(default)]
    receive_inbox: Option<hagency_core::received_files::ReceiveInboxPlan>,
    /// Existing verified Matrix session IDs whose timelines the continuous
    /// driver polls. An empty list permits host-queued dispatches only.
    #[serde(default)]
    intake_sessions: Vec<String>,
    /// Continuous agent sessions whose verified wake messages become owned
    /// tasks. Each route names the retained workspace used for its dispatch.
    #[serde(default)]
    agent_inboxes: Vec<hagency_core::agent_inbox::AgentInboxPlan>,
    executable: PathBuf,
    executable_sha256: String,
    #[serde(deserialize_with = "workspace_map")]
    workspaces: BTreeMap<String, PathBuf>,
    file_limit: usize,
    operation_ms: u64,
    response_ms: u64,
    #[serde(default = "default_approval_wait")]
    approval_owner_wait_ms: u64,
    #[serde(default)]
    matrix_request_interval_ms: Option<u64>,
    /// Original SDK/enrollment budget, selected before any Matrix owner exists.
    #[serde(default)]
    matrix_sdk_timeout_ms: Option<u64>,
    matrix: Matrix,
    #[serde(default)]
    approval: Option<ApprovalMatrix>,
    #[serde(default)]
    factory_service: Option<FactoryService>,
}
fn default_approval_wait() -> u64 {
    1000
}
fn matrix_limits(
    origin: &str,
    interval: Option<u64>,
    sdk_ms: Option<u64>,
) -> Result<hagency_matrix::Limits, Failure> {
    let mut limits = hagency_matrix::Limits {
        request_pacing: interval
            .map(|ms| {
                hagency_matrix::RequestPacing::new(origin, std::time::Duration::from_millis(ms))
                    .map(std::sync::Arc::new)
                    .map_err(|_| Failure::Config)
            })
            .transpose()?,
        ..hagency_matrix::Limits::default()
    };
    if let Some(ms) = sdk_ms {
        if !(10..=60_000).contains(&ms) {
            return Err(Failure::Config);
        }
        limits.sdk = std::time::Duration::from_millis(ms);
    }
    Ok(limits)
}
fn approval_host(wait: u64, limits: Limits) -> Result<hagency_execution::ApprovalHost, Failure> {
    // After an unanswered owner wait the host still has to record the expiry
    // and send the decline (ADR046 amendment): one durable write, the existing
    // authorize/begin/check round trips and the frame all fit in this reserve.
    let reserve = limits.response_ms.max(5000);
    if wait
        .checked_add(reserve)
        .is_none_or(|n| n > limits.operation_ms)
    {
        return Err(Failure::Config);
    }
    hagency_execution::ApprovalHost::new(8, 2, wait, reserve).map_err(|_| Failure::Config)
}

#[cfg(test)]
mod approval_wait_tests {
    use super::*;
    #[test]
    fn native_matrix_pacing_configuration() {
        assert!(
            matrix_limits("https://example.test/", None, None)
                .unwrap()
                .request_pacing
                .is_none()
        );
        for ms in [0, 9, 1001, u64::MAX] {
            assert!(matrix_limits("https://example.test/", Some(ms), None).is_err());
        }
        let limits = matrix_limits("https://example.test/", Some(250), None).unwrap();
        let clone = limits.clone();
        assert!(std::sync::Arc::ptr_eq(
            limits.request_pacing.as_ref().unwrap(),
            clone.request_pacing.as_ref().unwrap()
        ));
    }
    #[test]
    fn native_matrix_sdk_budget_configuration() {
        let original = matrix_limits("https://example.test/", None, None).unwrap();
        assert_eq!(original.sdk, std::time::Duration::from_secs(20));
        for ms in [0, 9, 60_001, u64::MAX] {
            assert!(matrix_limits("https://example.test/", None, Some(ms)).is_err());
        }
        for ms in [10, 20_000, 60_000] {
            let limits = matrix_limits("https://example.test/", Some(1000), Some(ms)).unwrap();
            assert_eq!(limits.clone().sdk, std::time::Duration::from_millis(ms));
            assert_eq!(limits.connect, original.connect);
            assert_eq!(limits.headers, original.headers);
            assert_eq!(limits.request, original.request);
            assert_eq!(limits.body_idle, original.body_idle);
        }
    }
    #[test]
    fn native_bootstrap_approval_wait_bound() {
        let limits = Limits {
            operation_ms: 30000,
            response_ms: 2000,
        };
        assert_eq!(default_approval_wait(), 1000);
        assert!(approval_host(default_approval_wait(), limits).is_ok());
        assert!(approval_host(10000, limits).is_ok());
        assert!(approval_host(25000, limits).is_ok());
        for wait in [0, 25001, u64::MAX] {
            assert!(approval_host(wait, limits).is_err());
        }
        assert!(
            approval_host(
                1000,
                Limits {
                    operation_ms: 2500,
                    response_ms: 1500
                }
            )
            .is_err()
        );
        let long = Limits {
            operation_ms: hagency_core::tasks::MAX_OWNED_OPERATION_MS,
            response_ms: 2000,
        };
        assert!(long.validate());
        assert_eq!(
            long.capability_ms().unwrap(),
            hagency_core::tasks::MAX_OWNED_CAPABILITY_MS
        );
        for wait in [1000, 60_000, 595_000] {
            assert!(approval_host(wait, long).is_ok());
        }
        for wait in [0, 595_001, u64::MAX] {
            assert!(approval_host(wait, long).is_err());
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactoryService {
    profile: String,
    idle_ms: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalCodex {
    profile: String,
    preset: String,
    seat: String,
    home: PathBuf,
    codex_home: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Matrix {
    #[serde(default)]
    token_provisioning: Option<TokenProvisioning>,
    #[serde(default)]
    crypto_enrollment: Option<CryptoEnrollment>,
    origin: String,
    server_name: String,
    registration_fingerprint: String,
    engagement_id: String,
    registration_generation: u64,
    transport_generation: u64,
    sender_mxid: String,
    device_id: String,
    rooms: Vec<Room>,
}
#[derive(Deserialize)]
#[serde(tag = "profile", deny_unknown_fields)]
enum TokenProvisioning {
    #[serde(rename = "registration_token_account_step_v1")]
    Account {},
    #[serde(rename = "registration_token_rooms_enrollment_step_v1")]
    Rooms { peer_masters: Vec<PeerMaster> },
    #[serde(rename = "registration_token_home_rooms_enrollment_step_v1")]
    HomeRooms {
        peer_masters: Vec<PeerMaster>,
        home: HomeConfiguration,
    },
    #[serde(rename = "appservice_login_home_rooms_enrollment_step_v1")]
    AppserviceHomeRooms {
        peer_masters: Vec<PeerMaster>,
        home: HomeConfiguration,
        namespace_prefix: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HomeConfiguration {
    root: PathBuf,
    task_client: PathBuf,
    projects: Vec<hagency_store::agent_home::HomeProject>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CryptoEnrollment {
    profile: String,
    peer_masters: Vec<PeerMaster>,
}
/// The approval bot's own credential set (PC-C0, plan v4 Q3): a SECOND
/// identity, token, device and SDK root, never the pooled ordinary
/// `HostConfig` (which `Collector::new` refuses for `approval == true`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalMatrix {
    origin: String,
    server_name: String,
    registration_fingerprint: String,
    engagement_id: String,
    registration_generation: u64,
    transport_generation: u64,
    sender_mxid: String,
    device_id: String,
    rooms: Vec<Room>,
    peer_masters: Vec<PeerMaster>,
}
/// What `bootstrap::approval` builds the pump's collector from.
pub(super) struct Approval {
    pub config: HostConfig,
    pub engagement_id: String,
    pub anchors: Vec<(String, String)>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerMaster {
    user_id: String,
    master_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Room {
    id: String,
    generation: u64,
    privacy: RoomPrivacy,
}
pub(super) struct Prepared {
    pub host: Host,
    pub managed_account: Option<String>,
    pub matrix: Option<HostConfig>,
    pub approval: Option<Approval>,
    pub provisioning: Option<hagency_matrix::TokenProvisioningHost>,
    pub warm: Option<hagency_execution::WarmHostPlan>,
    pub fleet: Option<super::fleet::Setup>,
    pub files: Option<crate::file_service::Setup>,
    pub receives: Option<crate::receive_service::Setup>,
    pub enrollment: bool,
    pub receive_inbox: Option<hagency_core::received_files::ReceiveInboxPlan>,
    pub intake_sessions: Vec<String>,
    pub agent_inboxes: Vec<hagency_core::agent_inbox::AgentInboxPlan>,
    pub claim: OwnedClaimProfile,
    pub limits: Limits,
    pub max_live: u32,
    #[cfg(test)]
    pub discard_claim_reply: bool,
}
pub(super) fn read(path: &Path, limit: usize) -> Result<Vec<u8>, Failure> {
    let file = private::open(path, false).map_err(|_| Failure::Config)?;
    let length = usize::try_from(file.metadata().map_err(|_| Failure::Config)?.len())
        .map_err(|_| Failure::Config)?;
    if length > limit {
        return Err(Failure::Config);
    }
    let mut bytes = Vec::with_capacity(length);
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Config)?;
    if bytes.len() > limit {
        return Err(Failure::Config);
    }
    Ok(bytes)
}
/// Fixed-memory digest under trusted executable/ancestor provisioning. This is
/// not handle-based executable launch or a claim of hostile namespace isolation.
fn verify_executable(path: &Path, expected: &str) -> Result<(), Failure> {
    if !path.is_absolute()
        || path.as_os_str().as_encoded_bytes().len() > 4096
        || path.canonicalize().ok().as_deref() != Some(path)
        || expected.len() != 64
        || !expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Failure::Config);
    }
    let before = std::fs::symlink_metadata(path).map_err(|_| Failure::Config)?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.len() == 0
        || before.len() > EXECUTABLE_BYTES
    {
        return Err(Failure::Config);
    }
    let mut file = File::open(path).map_err(|_| Failure::Config)?;
    let actual = file.metadata().map_err(|_| Failure::Config)?;
    if !actual.is_file() || actual.len() != before.len() {
        return Err(Failure::Config);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 32 * 1024];
    let mut total = 0u64;
    tracing::trace!(target: "hagency_startup_observation", "native startup boundary: executable_hash_entered");
    loop {
        let count = file.read(&mut buffer).map_err(|_| Failure::Config)?;
        if count == 0 {
            break;
        }
        total = total.checked_add(count as u64).ok_or(Failure::Config)?;
        if total > EXECUTABLE_BYTES || total > actual.len() {
            return Err(Failure::Config);
        }
        hasher.update(&buffer[..count]);
    }
    if total != actual.len() || file.metadata().map_err(|_| Failure::Config)?.len() != total {
        return Err(Failure::Config);
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect();
    tracing::trace!(target: "hagency_startup_observation", "native startup boundary: executable_hash_completed");
    if digest != expected {
        return Err(Failure::Config);
    }
    Ok(())
}
impl Prepared {
    pub(super) fn attach_factory(
        &mut self,
        approvals: Option<std::sync::Arc<hagency_matrix::ApprovalCollector>>,
    ) -> Result<(), Failure> {
        if let Some(mut provisioning) = self.provisioning.take() {
            if let Some(warm) = self.warm.take() {
                provisioning = provisioning
                    .with_warm_runtime(warm, approvals.ok_or(Failure::Config)?)
                    .map_err(|_| Failure::Config)?;
            }
            self.matrix = Some(
                self.matrix
                    .take()
                    .ok_or(Failure::Config)?
                    .with_token_account_provisioning(provisioning)
                    .map_err(|_| Failure::Config)?,
            );
        } else if self.warm.is_some() || self.fleet.is_some() {
            return Err(Failure::Config);
        }
        Ok(())
    }
    pub(super) fn load(
        state: &Path,
        address: SocketAddr,
        mode: DriverMode,
    ) -> Result<Self, Failure> {
        let (file, profile) = match mode {
            DriverMode::OneAttempt => {
                ("development-driver.json", "codex_app_server_development_v1")
            }
            DriverMode::Continuous => ("agent-driver.json", "codex_app_server_agent_v1"),
            DriverMode::Disabled => return Err(Failure::Config),
        };
        let bytes = read(&state.join(file), CONFIG_BYTES)?;
        let mut config: Config = serde_json::from_slice(&bytes).map_err(|error| {
            // This document contains paths and public Matrix identities only;
            // credentials remain in separate private files. Retain serde's
            // location so an operator can repair malformed deployment input
            // without weakening the fail-closed error at this boundary.
            tracing::error!(
                line = error.line(),
                column = error.column(),
                "invalid agent driver configuration"
            );
            Failure::Config
        })?;
        let enrollment = config.matrix.crypto_enrollment.is_some();
        if let Some(local) = &config.local_codex
            && (local.profile != "provider_owned_codex_v1" || config.managed_account.is_some())
        {
            return Err(Failure::Config);
        }
        if let Some(factory) = &config.factory_service
            && (mode != DriverMode::Continuous
                || factory.profile != "inline_factory_service_checkpoint_v1"
                || !(100..=1_200_000).contains(&factory.idle_ms)
                || config.approval.is_none()
                || !matches!(
                    &config.matrix.token_provisioning,
                    Some(
                        TokenProvisioning::HomeRooms { .. }
                            | TokenProvisioning::AppserviceHomeRooms { .. }
                    )
                ))
        {
            return Err(Failure::Config);
        }
        if config.profile != profile
            || config.workspaces.is_empty()
            || config.workspaces.len() > 16
            || config.matrix.rooms.is_empty()
            || config.matrix.rooms.len() > 16
            || !config.matrix.origin.starts_with("https://")
        {
            return Err(Failure::Config);
        }
        if mode == DriverMode::Continuous && config.receive_inbox.is_some() {
            // The fixed receive-inbox plan is deliberately one-dispatch-only;
            // it cannot be replayed as a scheduler input.
            return Err(Failure::Config);
        }
        if mode == DriverMode::OneAttempt && !config.agent_inboxes.is_empty() {
            return Err(Failure::Config);
        }
        if config.agent_inboxes.len() > 16 {
            return Err(Failure::Config);
        }
        let mut inbox_sessions = std::collections::BTreeSet::new();
        for plan in &config.agent_inboxes {
            plan.validate().map_err(|_| Failure::Config)?;
            if !config.workspaces.contains_key(&plan.workspace_id)
                || !inbox_sessions.insert(&plan.session_id)
            {
                return Err(Failure::Config);
            }
        }
        let mut combined_sessions = config.intake_sessions.clone();
        for plan in &config.agent_inboxes {
            if !combined_sessions.contains(&plan.session_id) {
                combined_sessions.push(plan.session_id.clone());
            }
        }
        if !combined_sessions.is_empty() {
            hagency_matrix::HostIntakePlan::new(combined_sessions.clone())
                .map_err(|_| Failure::Config)?;
        }
        config.intake_sessions = combined_sessions;
        if config.factory_service.is_some() && config.intake_sessions.is_empty() {
            // Reception provisioning is read by the coordinator's actual
            // intake. A fleet with no verified intake session cannot run it.
            return Err(Failure::Config);
        }
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: executable_verify_entered");
        verify_executable(&config.executable, &config.executable_sha256)?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: executable_verify_completed");
        let own = std::env::current_exe()
            .map_err(|_| Failure::Config)?
            .canonicalize()
            .map_err(|_| Failure::Config)?;
        let mut environment = BTreeMap::new();
        if config.local_codex.is_none() {
            let runtime_home = state.join("runtime-home");
            private::directory(&runtime_home).map_err(|_| Failure::Config)?;
            environment.insert("HOME".into(), runtime_home.clone().into_os_string());
            environment.insert("CODEX_HOME".into(), runtime_home.into_os_string());
        }
        if let Some(system) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system);
        }
        let transport = MatrixTransportObservation {
            engagement_id: config.matrix.engagement_id,
            registration_generation: config.matrix.registration_generation,
            generation: config.matrix.transport_generation,
            sender_mxid: config.matrix.sender_mxid,
            device_id: config.matrix.device_id,
        };
        let mut claim_rooms = Vec::new();
        let mut rooms = Vec::new();
        for room in config.matrix.rooms {
            claim_rooms.push(
                OwnedClaimRoom::new(room.id.clone(), room.generation, room.privacy.clone())
                    .map_err(|_| Failure::Config)?,
            );
            rooms.push(HostRoom {
                room_id: room.id,
                generation: room.generation,
                privacy: room.privacy,
            });
        }
        let mut claim = OwnedClaimProfile::new(
            transport.clone(),
            claim_rooms,
            config.workspaces.keys().cloned().collect(),
        )
        .map_err(|_| Failure::Config)?;
        if let Some(plan) = &config.receive_inbox {
            plan.validate().map_err(|_| Failure::Config)?;
            if !config.workspaces.contains_key(&plan.workspace_id) {
                return Err(Failure::Config);
            }
            claim = claim
                .restrict_dispatch(plan.dispatch_id.clone())
                .map_err(|_| Failure::Config)?;
        }
        let mut host = Host::new(
            own.clone(),
            config.executable.clone(),
            environment.clone(),
            config.workspaces,
        )
        .and_then(|h| h.with_file_limit(config.file_limit))
        .and_then(|h| h.with_task_helper(own.clone(), address))
        .map_err(|_| Failure::Config)?;
        let uses_local_codex = config.local_codex.is_some();
        if let Some(local) = config.local_codex {
            claim = claim
                .restrict_resource(local.preset.clone(), local.seat.clone())
                .map_err(|_| Failure::Config)?;
            let local = hagency_execution::LocalCodex::new(
                local.preset,
                local.seat,
                local.home,
                local.codex_home,
            )
            .map_err(|_| Failure::Config)?;
            host = host.with_local_codex(local).map_err(|_| Failure::Config)?;
        }
        if config.send_file {
            host = host.with_file_tools().map_err(|_| Failure::Config)?;
        }
        if config.receive_file {
            hagency_core::received_files::receive_limit(config.file_limit)
                .map_err(|_| Failure::Config)?;
            host = host.with_receive_tools().map_err(|_| Failure::Config)?;
        }
        if config.coordination_tools {
            host = host
                .with_coordination_tools()
                .map_err(|_| Failure::Config)?;
        }
        let namespace = hagency_core::canonical::digest(&serde_json::json!([
            "native_file_storage_v1",
            config.matrix.origin,
            config.matrix.server_name,
            config.matrix.registration_fingerprint,
            transport.engagement_id,
            transport.registration_generation,
            transport.sender_mxid,
            transport.device_id
        ]))
        .map_err(|_| Failure::Config)?;
        let files = config.send_file.then(|| crate::file_service::Setup {
            directory: state.join("file-media"),
            namespace,
            limit: config.file_limit,
        });
        let token = read(&state.join("matrix.access_token"), 4096)?;
        let token = std::str::from_utf8(&token).map_err(|_| Failure::Config)?;
        let key: [u8; 32] = read(&state.join("matrix.sdk_key"), 32)?
            .try_into()
            .map_err(|_| Failure::Config)?;
        let matrix_limits = matrix_limits(
            &config.matrix.origin,
            config.matrix_request_interval_ms,
            config.matrix_sdk_timeout_ms,
        )?;
        let mut matrix = HostConfig::new(
            HostIdentity {
                server_name: config.matrix.server_name,
                registration_fingerprint: config.matrix.registration_fingerprint,
                transport,
            },
            &config.matrix.origin,
            token,
            state.join("sdk"),
            key,
            rooms,
            matrix_limits.clone(),
        )
        .map_err(|_| Failure::Config)?;
        // Fail-closed: the pre-project reception room comes from the store's
        // recorded registration for this host's engagement. A host whose
        // engagement names no registration (or a registration with no
        // reception room) refuses to start rather than observing nothing and
        // silently dropping the provisioning ingress.
        let mut provisioning = None;
        {
            let engagement_id = matrix.engagement_id().to_owned();
            let registration = DomainRepository::open(state)
                .map_err(|_| Failure::Config)?
                .provisioning_registration_for_engagement(&engagement_id)
                .map_err(|_| Failure::Config)?;
            matrix
                .with_reception_room(HostRoom {
                    room_id: registration.reception_room_id.clone(),
                    generation: registration.generation,
                    privacy: RoomPrivacy::Group {},
                })
                .map_err(|_| Failure::Config)?;
            if let Some(profile) = config.matrix.token_provisioning {
                let (peer_masters, home, as_namespace) = match profile {
                    TokenProvisioning::Account {} => (None, None, None),
                    TokenProvisioning::Rooms { peer_masters } => (Some(peer_masters), None, None),
                    TokenProvisioning::HomeRooms { peer_masters, home } => {
                        (Some(peer_masters), Some(home), None)
                    }
                    TokenProvisioning::AppserviceHomeRooms {
                        peer_masters,
                        home,
                        namespace_prefix,
                    } => (Some(peer_masters), Some(home), Some(namespace_prefix)),
                };
                let token = if as_namespace.is_some() {
                    read(&state.join("matrix.appservice_token"), 4096)?
                } else {
                    read(&state.join("matrix.registration_token"), 64)?
                };
                let token = std::str::from_utf8(&token).map_err(|_| Failure::Config)?;
                let key: [u8; 32] = read(&state.join("matrix.provisioning_key"), 32)?
                    .try_into()
                    .map_err(|_| Failure::Config)?;
                let mut host = if let Some(namespace) = as_namespace {
                    hagency_matrix::TokenProvisioningHost::application_service(
                        registration.clone(),
                        &config.matrix.origin,
                        hagency_matrix::ApplicationServiceCredential::new(token, &namespace)
                            .map_err(|_| Failure::Config)?,
                        state.to_owned(),
                        key,
                        matrix_limits.clone(),
                    )
                } else {
                    hagency_matrix::TokenProvisioningHost::new(
                        registration.clone(),
                        &config.matrix.origin,
                        token,
                        state.to_owned(),
                        key,
                        matrix_limits.clone(),
                    )
                }
                .map_err(|_| Failure::Config)?;
                if let Some(peer_masters) = peer_masters {
                    let token = read(&state.join("matrix.representative_token"), 4096)?;
                    let token = std::str::from_utf8(&token).map_err(|_| Failure::Config)?;
                    host = host
                        .with_agent_rooms_enrollment(
                            token,
                            peer_masters
                                .into_iter()
                                .map(|p| (p.user_id, p.master_key))
                                .collect(),
                        )
                        .map_err(|_| Failure::Config)?;
                }
                if let Some(home) = home {
                    let plan = hagency_store::agent_home::ManagedHomePlan::new(
                        home.root,
                        home.projects,
                        home.task_client,
                    )
                    .map_err(|_| Failure::Config)?;
                    host = host.with_managed_homes(plan).map_err(|_| Failure::Config)?;
                }
                let ca = state.join("matrix.ca.pem");
                if ca.try_exists().map_err(|_| Failure::Config)? {
                    host = host
                        .with_root_pem(&read(&ca, 16 * 1024)?)
                        .map_err(|_| Failure::Config)?;
                }
                provisioning = Some(host);
            }
        }
        if let Some(profile) = config.matrix.crypto_enrollment {
            if profile.profile != "fresh_own_account_v1" {
                return Err(Failure::Config);
            }
            matrix = matrix
                .with_fresh_account_enrollment(
                    profile
                        .peer_masters
                        .into_iter()
                        .map(|p| (p.user_id, p.master_key))
                        .collect(),
                )
                .map_err(|_| Failure::Config)?;
        }
        let ca = state.join("matrix.ca.pem");
        if ca.try_exists().map_err(|_| Failure::Config)? {
            matrix = matrix
                .with_root_pem(&read(&ca, 16 * 1024)?)
                .map_err(|_| Failure::Config)?;
        }
        let limits = Limits {
            operation_ms: config.operation_ms,
            response_ms: config.response_ms,
        };
        if !limits.validate() {
            return Err(Failure::Config);
        }
        // The approval bot's own credential and the host's approval capacity
        // (PC-C0, plan v4 Q3): a SECOND identity set, never the pooled
        // ordinary `HostConfig`, and the `ApprovalHost` without whose
        // attachment `Operation::start_mode` never creates the notices
        // channel at all. `ApprovalHost::new` values must `fits(limits)` or
        // every start refuses with `Failure::Admission`.
        let mut runtime_approvals = None;
        let owner_wait_ms = config.approval_owner_wait_ms;
        let approval = match config.approval {
            Some(approval) => {
                if approval.rooms.is_empty()
                    || approval.rooms.len() > 16
                    || !approval.origin.starts_with("https://")
                    || !approval
                        .rooms
                        .iter()
                        .all(|r| matches!(r.privacy, RoomPrivacy::Direct { .. }))
                {
                    return Err(Failure::Config);
                }
                let transport = MatrixTransportObservation {
                    engagement_id: approval.engagement_id.clone(),
                    registration_generation: approval.registration_generation,
                    generation: approval.transport_generation,
                    sender_mxid: approval.sender_mxid.clone(),
                    device_id: approval.device_id.clone(),
                };
                let rooms = approval
                    .rooms
                    .into_iter()
                    .map(|room| HostRoom {
                        room_id: room.id,
                        generation: room.generation,
                        privacy: room.privacy,
                    })
                    .collect();
                let token = read(&state.join("approval.access_token"), 4096)?;
                let token = std::str::from_utf8(&token).map_err(|_| Failure::Config)?;
                let key: [u8; 32] = read(&state.join("approval.sdk_key"), 32)?
                    .try_into()
                    .map_err(|_| Failure::Config)?;
                let mut config = HostConfig::new(
                    HostIdentity {
                        server_name: approval.server_name,
                        registration_fingerprint: approval.registration_fingerprint,
                        transport,
                    },
                    &approval.origin,
                    token,
                    state.join("approval-sdk"),
                    key,
                    rooms,
                    matrix_limits.clone(),
                )
                .map_err(|_| Failure::Config)?;
                let ca = state.join("approval.ca.pem");
                if ca.try_exists().map_err(|_| Failure::Config)? {
                    config = config
                        .with_root_pem(&read(&ca, 16 * 1024)?)
                        .map_err(|_| Failure::Config)?;
                }
                // The capacity must fit the operation limits exactly as
                // `ApprovalHost::fits` checks them, or `start_mode` refuses.
                let host_approvals = approval_host(owner_wait_ms, limits)?;
                host = host
                    .with_approvals(host_approvals.clone())
                    .map_err(|_| Failure::Config)?;
                runtime_approvals = Some(host_approvals);
                Some(Approval {
                    config,
                    engagement_id: approval.engagement_id,
                    anchors: approval
                        .peer_masters
                        .into_iter()
                        .map(|p| (p.user_id, p.master_key))
                        .collect(),
                })
            }
            None => None,
        };
        let (warm, fleet) = if let Some(factory) = config.factory_service {
            let contexts = state.join("factory-task-contexts");
            private::directory(&contexts).map_err(|_| Failure::Config)?;
            let bridge = hagency_execution::WarmTaskBridge::new(own.clone(), address, contexts)
                .map_err(|_| Failure::Config)?;
            let initialize = Limits {
                operation_ms: limits.operation_ms.min(30_000),
                response_ms: limits.response_ms,
            };
            let mut warm = hagency_execution::WarmHostPlan::new(
                own,
                config.executable,
                environment,
                bridge,
                runtime_approvals.ok_or(Failure::Config)?,
                hagency_execution::WarmLimits {
                    initialize,
                    idle_ms: factory.idle_ms,
                },
            )
            .and_then(|plan| {
                plan.with_file_access(config.file_limit, config.send_file, config.receive_file)
            })
            .map(|plan| {
                if config.coordination_tools {
                    plan.with_coordination_tools()
                } else {
                    plan
                }
            })
            .map_err(|_| Failure::Config)?;
            if uses_local_codex {
                warm = warm
                    .with_local_codex_from_host(&host)
                    .map_err(|_| Failure::Config)?;
            }
            (
                Some(warm),
                Some(super::fleet::Setup {
                    state: state.to_owned(),
                    limit: config.file_limit,
                    send: config.send_file,
                    receive: config.receive_file,
                    limits,
                }),
            )
        } else {
            (None, None)
        };
        let max_live = if warm.is_some() { 8 } else { 1 };
        Ok(Self {
            host,
            managed_account: config.managed_account,
            matrix: Some(matrix),
            approval,
            provisioning,
            warm,
            fleet,
            files,
            receives: config
                .receive_file
                .then_some(crate::receive_service::Setup {
                    limit: config.file_limit,
                }),
            enrollment,
            receive_inbox: config.receive_inbox,
            intake_sessions: config.intake_sessions,
            agent_inboxes: config.agent_inboxes,
            claim,
            limits,
            max_live,
            #[cfg(test)]
            discard_claim_reply: false,
        })
    }
}

fn workspace_map<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, PathBuf>, D::Error> {
    struct Map;
    impl<'de> serde::de::Visitor<'de> for Map {
        type Value = BTreeMap<String, PathBuf>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most sixteen unique workspace IDs")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((id, path)) = map.next_entry::<String, PathBuf>()? {
                if result.len() >= 16 || result.insert(id, path).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate workspace or capacity exceeded",
                    ));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Map)
}
