use super::Failure;
use hagency_core::replies::{MatrixTransportObservation, RoomPrivacy};
use hagency_execution::{Host, Limits};
use hagency_matrix::{HostConfig, HostIdentity, HostRoom};
use hagency_store::{OwnedClaimProfile, OwnedClaimRoom, private};
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
    send_file: bool,
    #[serde(default)]
    receive_file: bool,
    #[serde(default)]
    receive_inbox: Option<hagency_core::received_files::ReceiveInboxPlan>,
    executable: PathBuf,
    executable_sha256: String,
    #[serde(deserialize_with = "workspace_map")]
    workspaces: BTreeMap<String, PathBuf>,
    file_limit: usize,
    operation_ms: u64,
    response_ms: u64,
    matrix: Matrix,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Matrix {
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
#[serde(deny_unknown_fields)]
struct CryptoEnrollment {
    profile: String,
    peer_masters: Vec<PeerMaster>,
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
    pub files: Option<crate::file_service::Setup>,
    pub receives: Option<crate::receive_service::Setup>,
    pub enrollment: bool,
    pub receive_inbox: Option<hagency_core::received_files::ReceiveInboxPlan>,
    pub claim: OwnedClaimProfile,
    pub limits: Limits,
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
    pub(super) fn load(state: &Path, address: SocketAddr) -> Result<Self, Failure> {
        let bytes = read(&state.join("development-driver.json"), CONFIG_BYTES)?;
        let config: Config = serde_json::from_slice(&bytes).map_err(|_| Failure::Config)?;
        let enrollment = config.matrix.crypto_enrollment.is_some();
        if config.profile != "codex_app_server_development_v1"
            || config.workspaces.is_empty()
            || config.workspaces.len() > 16
            || config.matrix.rooms.is_empty()
            || config.matrix.rooms.len() > 16
            || !config.matrix.origin.starts_with("https://")
        {
            return Err(Failure::Config);
        }
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: executable_verify_entered");
        verify_executable(&config.executable, &config.executable_sha256)?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: executable_verify_completed");
        let own = std::env::current_exe()
            .map_err(|_| Failure::Config)?
            .canonicalize()
            .map_err(|_| Failure::Config)?;
        let runtime_home = state.join("runtime-home");
        private::directory(&runtime_home).map_err(|_| Failure::Config)?;
        let mut environment = BTreeMap::from([
            ("HOME".into(), runtime_home.clone().into_os_string()),
            ("CODEX_HOME".into(), runtime_home.into_os_string()),
        ]);
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
            config.executable,
            environment,
            config.workspaces,
        )
        .and_then(|h| h.with_file_limit(config.file_limit))
        .and_then(|h| h.with_task_helper(own, address))
        .map_err(|_| Failure::Config)?;
        if config.send_file {
            host = host.with_file_tools().map_err(|_| Failure::Config)?;
        }
        if config.receive_file {
            hagency_core::received_files::receive_limit(config.file_limit)
                .map_err(|_| Failure::Config)?;
            host = host.with_receive_tools().map_err(|_| Failure::Config)?;
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
            hagency_matrix::Limits::default(),
        )
        .map_err(|_| Failure::Config)?;
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
        if !(100..=30_000).contains(&limits.operation_ms)
            || !(10..=2000).contains(&limits.response_ms)
            || limits.response_ms > limits.operation_ms
        {
            return Err(Failure::Config);
        }
        Ok(Self {
            host,
            managed_account: config.managed_account,
            matrix: Some(matrix),
            files,
            receives: config
                .receive_file
                .then_some(crate::receive_service::Setup {
                    limit: config.file_limit,
                }),
            enrollment,
            receive_inbox: config.receive_inbox,
            claim,
            limits,
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
