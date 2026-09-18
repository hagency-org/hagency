//! Operator-owned adoption of an already-created Matrix agent. This is the
//! fresh-state bridge for deployments whose account and rooms were created by
//! an external administrator: every authority fact is re-read from Matrix and
//! verified before the durable provision effect is completed.
use hagency_core::{
    authority::{
        ProjectRequest, Registration, RequestObservation, RoomObservation, SourceObservation,
        verify_request,
    },
    canonical,
    project::Resource,
    replies::{MatrixRoomObservation, MatrixTransportObservation, RoomPrivacy},
    tasks::SessionBinding,
};
use hagency_store::{DomainRepository, EffectOutcome, EffectState, Repository, private};
use reqwest::{StatusCode, Url, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
#[path = "provision/tests.rs"]
mod tests;

const DOCUMENT_LIMIT: u64 = 64 * 1024;
const RESPONSE_LIMIT: u64 = 1024 * 1024;

#[derive(clap::Subcommand)]
pub enum Command {
    /// Adopt an external Matrix account, owner DM and optional project inbox.
    Existing {
        /// Non-secret provisioning description.
        #[arg(long)]
        file: PathBuf,
        /// Private token for the owner/observer that can read all authority rooms.
        #[arg(long)]
        observer_token: PathBuf,
        /// Private token for the agent account being adopted.
        #[arg(long)]
        agent_token: PathBuf,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Existing {
    origin: String,
    observer_mxid: String,
    observer_device_id: String,
    agent_mxid: String,
    agent_device_id: String,
    agent_room_id: String,
    /// A failed/closed transport is never revived at its old incarnation.
    /// The operator supplies the next generation after fresh authentication.
    #[serde(default = "initial_generation")]
    transport_generation: u64,
    #[serde(default = "initial_generation")]
    room_generation: u64,
    session_id: String,
    workspace_id: String,
    #[serde(default)]
    project_inbox: Option<ProjectInbox>,
    registration: Registration,
    resource: Resource,
    request: ProjectRequest,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectInbox {
    session_id: String,
    workspace_id: String,
    #[serde(default = "initial_generation")]
    room_generation: u64,
}
fn initial_generation() -> u64 {
    1
}

#[derive(Serialize)]
pub struct Receipt {
    pub engagement_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub matrix_observation_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_inbox: Option<ProjectInbox>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("private provisioning input is unavailable")]
    Private,
    #[error("provisioning document is invalid")]
    Document,
    #[error("Matrix provisioning observation was refused")]
    Matrix,
    #[error("Matrix provisioning authority did not match")]
    Authority,
    #[error("native provisioning state was refused")]
    State,
    #[error("native provisioning state was refused at {0}")]
    StateAt(&'static str),
}

struct Matrix {
    client: reqwest::Client,
    origin: Url,
    token: String,
}

impl Matrix {
    fn new(origin: &str, token: Vec<u8>) -> Result<Self, Error> {
        let token = String::from_utf8(token).map_err(|_| Error::Private)?;
        if token.len() < 16
            || token.len() > 512
            || !token.bytes().all(|byte| (33..=126).contains(&byte))
        {
            return Err(Error::Private);
        }
        let origin = Url::parse(origin).map_err(|_| Error::Document)?;
        if origin.scheme() != "https"
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !origin.username().is_empty()
            || origin.password().is_some()
        {
            return Err(Error::Document);
        }
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| Error::Matrix)?;
        Ok(Self {
            client,
            origin,
            token,
        })
    }

    async fn get(&self, segments: &[&str]) -> Result<Value, Error> {
        let mut url = self.origin.clone();
        url.path_segments_mut()
            .map_err(|_| Error::Document)?
            .extend(segments);
        let mut response = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|_| Error::Matrix)?;
        if response.status() != StatusCode::OK
            || response
                .content_length()
                .is_some_and(|n| n > RESPONSE_LIMIT)
        {
            return Err(Error::Matrix);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Matrix)? {
            if bytes
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n as u64 > RESPONSE_LIMIT)
            {
                return Err(Error::Matrix);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Matrix)
    }

    async fn identity(&self, user: &str, device: &str) -> Result<Value, Error> {
        let value = self
            .get(&["_matrix", "client", "v3", "account", "whoami"])
            .await?;
        if value.get("user_id").and_then(Value::as_str) != Some(user)
            || value.get("device_id").and_then(Value::as_str) != Some(device)
            || value
                .get("is_guest")
                .is_some_and(|guest| guest != &Value::Bool(false))
        {
            return Err(Error::Authority);
        }
        Ok(value)
    }

    async fn event(&self, room: &str, event: &str) -> Result<Value, Error> {
        self.get(&["_matrix", "client", "v3", "rooms", room, "event", event])
            .await
    }

    async fn room(&self, room: &str, server: &str) -> Result<ObservedRoom, Error> {
        let value = self
            .get(&["_matrix", "client", "v3", "rooms", room, "state"])
            .await?;
        ObservedRoom::parse(room, server, value)
    }
}

struct ObservedRoom {
    joined: BTreeSet<String>,
    invite_only: bool,
    encrypted: bool,
    powers: BTreeMap<String, i64>,
    default_power: i64,
    invite_power: i64,
    binding: Option<Value>,
    name: Option<String>,
}

impl ObservedRoom {
    fn parse(room: &str, server: &str, value: Value) -> Result<Self, Error> {
        let events = value.as_array().ok_or(Error::Matrix)?;
        if events.len() > 4096 {
            return Err(Error::Matrix);
        }
        let mut states = BTreeSet::new();
        let mut joined = BTreeSet::new();
        let mut invite_only = false;
        let mut encrypted = false;
        let mut powers = BTreeMap::new();
        let mut default_power = 0;
        let mut invite_power = 0;
        let mut binding = None;
        let mut name = None;
        for event in events {
            let kind = event
                .get("type")
                .and_then(Value::as_str)
                .ok_or(Error::Matrix)?;
            let key = event
                .get("state_key")
                .and_then(Value::as_str)
                .ok_or(Error::Matrix)?;
            if !states.insert((kind.to_owned(), key.to_owned()))
                || event
                    .get("room_id")
                    .is_some_and(|value| value.as_str() != Some(room))
            {
                return Err(Error::Matrix);
            }
            let content = event
                .get("content")
                .and_then(Value::as_object)
                .ok_or(Error::Matrix)?;
            match kind {
                "m.room.member" => {
                    let suffix = key.split_once(':').map(|(_, value)| value);
                    if !key.starts_with('@') || suffix != Some(server) {
                        return Err(Error::Matrix);
                    }
                    if content.get("membership").and_then(Value::as_str) == Some("join") {
                        joined.insert(key.to_owned());
                    }
                }
                "m.room.join_rules" => {
                    if !key.is_empty() {
                        return Err(Error::Matrix);
                    }
                    invite_only =
                        content.get("join_rule").and_then(Value::as_str) == Some("invite");
                }
                "m.room.encryption" => {
                    if !key.is_empty()
                        || content.get("algorithm").and_then(Value::as_str)
                            != Some("m.megolm.v1.aes-sha2")
                    {
                        return Err(Error::Matrix);
                    }
                    encrypted = true;
                }
                "m.room.power_levels" => {
                    if !key.is_empty() {
                        return Err(Error::Matrix);
                    }
                    default_power = content
                        .get("users_default")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    invite_power = content.get("invite").and_then(Value::as_i64).unwrap_or(0);
                    for (user, level) in content
                        .get("users")
                        .and_then(Value::as_object)
                        .ok_or(Error::Matrix)?
                    {
                        powers.insert(user.clone(), level.as_i64().ok_or(Error::Matrix)?);
                    }
                }
                "m.room.name" => {
                    if !key.is_empty() {
                        return Err(Error::Matrix);
                    }
                    name = content
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                "com.hagency.project.binding.v1" => {
                    if !key.is_empty() {
                        return Err(Error::Matrix);
                    }
                    binding = Some(Value::Object(content.clone()));
                }
                _ => {}
            }
        }
        Ok(Self {
            joined,
            invite_only,
            encrypted,
            powers,
            default_power,
            invite_power,
            binding,
            name,
        })
    }

    fn authority(self, room_id: String) -> RoomObservation {
        RoomObservation {
            room_id,
            joined: self.joined,
            invite_only: self.invite_only,
            encryption: self.encrypted.then(|| "m.megolm.v1.aes-sha2".to_owned()),
            powers: self.powers,
            default_power: self.default_power,
            invite_power: self.invite_power,
            binding: self.binding,
            name: self.name,
        }
    }
}

fn read_document(path: &Path) -> Result<Existing, Error> {
    let file = private::open(path, false).map_err(|_| Error::Private)?;
    if file.metadata().map_err(|_| Error::Private)?.len() > DOCUMENT_LIMIT {
        return Err(Error::Document);
    }
    serde_json::from_reader(file).map_err(|_| Error::Document)
}

fn now() -> Result<u64, Error> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::State)?
        .as_millis() as u64)
}

pub async fn run(state: &Path, command: Command) -> Result<Receipt, Error> {
    let Command::Existing {
        file,
        observer_token,
        agent_token,
    } = command;
    private::read_secret(&state.join("operator.token")).map_err(|_| Error::Private)?;
    let input = read_document(&file)?;
    let observer = Matrix::new(
        &input.origin,
        private::read_secret(&observer_token).map_err(|_| Error::Private)?,
    )?;
    let agent = Matrix::new(
        &input.origin,
        private::read_secret(&agent_token).map_err(|_| Error::Private)?,
    )?;
    adopt(state, input, observer, agent).await
}

async fn adopt(
    state: &Path,
    input: Existing,
    observer: Matrix,
    agent: Matrix,
) -> Result<Receipt, Error> {
    input.registration.validate().map_err(|_| Error::Document)?;
    input.resource.validate().map_err(|_| Error::Document)?;
    hagency_core::replies::generation(input.transport_generation).map_err(|_| Error::Document)?;
    hagency_core::replies::generation(input.room_generation).map_err(|_| Error::Document)?;
    input
        .request
        .validate(&input.registration)
        .map_err(|_| Error::Document)?;
    for id in [&input.session_id, &input.workspace_id] {
        hagency_core::project::identifier(id, 128).map_err(|_| Error::Document)?;
    }
    if let Some(inbox) = &input.project_inbox {
        for id in [&inbox.session_id, &inbox.workspace_id] {
            hagency_core::project::identifier(id, 128).map_err(|_| Error::Document)?;
        }
        hagency_core::replies::generation(inbox.room_generation).map_err(|_| Error::Document)?;
        if inbox.session_id == input.session_id
            || inbox.workspace_id == input.workspace_id
            || input.agent_room_id == input.request.target_room_id
        {
            return Err(Error::Document);
        }
    }
    let observer_identity = observer
        .identity(&input.observer_mxid, &input.observer_device_id)
        .await?;
    let agent_identity = agent
        .identity(&input.agent_mxid, &input.agent_device_id)
        .await?;
    let source = observer
        .event(
            &input.request.source_room_id,
            &input.request.source_event_id,
        )
        .await?;
    let reception = observer
        .room(
            &input.request.source_room_id,
            &input.registration.server_name,
        )
        .await?
        .authority(input.request.source_room_id.clone());
    let project = observer
        .room(
            &input.request.target_room_id,
            &input.registration.server_name,
        )
        .await?
        .authority(input.request.target_room_id.clone());
    let owner_room = observer
        .room(
            &input.request.owner_dm_room_id,
            &input.registration.server_name,
        )
        .await?
        .authority(input.request.owner_dm_room_id.clone());
    let observed_agent_room = agent
        .room(&input.agent_room_id, &input.registration.server_name)
        .await?;
    let agent_room = MatrixRoomObservation {
        engagement_id: input.request.engagement_id().map_err(|_| Error::Document)?,
        registration_generation: input.registration.generation,
        transport_generation: input.transport_generation,
        room_id: input.agent_room_id.clone(),
        generation: input.room_generation,
        privacy: RoomPrivacy::Direct {
            human_mxid: input.request.owner_mxid.clone(),
        },
        joined: observed_agent_room.joined,
        invite_only: observed_agent_room.invite_only,
        encrypted: observed_agent_room.encrypted,
    };
    let project_room = if let Some(inbox) = &input.project_inbox {
        let observed = agent
            .room(
                &input.request.target_room_id,
                &input.registration.server_name,
            )
            .await?;
        if !observed.joined.contains(&input.agent_mxid)
            || !observed.joined.contains(&input.request.owner_mxid)
            || observed.joined != project.joined
            || observed.invite_only != project.invite_only
            || observed.encrypted != project.encryption.is_some()
        {
            return Err(Error::Authority);
        }
        Some(MatrixRoomObservation {
            engagement_id: agent_room.engagement_id.clone(),
            registration_generation: input.registration.generation,
            transport_generation: input.transport_generation,
            room_id: input.request.target_room_id.clone(),
            generation: inbox.room_generation,
            privacy: RoomPrivacy::Group {},
            joined: observed.joined,
            invite_only: observed.invite_only,
            encrypted: observed.encrypted,
        })
    } else {
        None
    };
    let observed_at = now()?;
    let proof = verify_request(
        &input.registration,
        input.request.clone(),
        RequestObservation {
            registration_generation: input.registration.generation,
            // Fresh authority time belongs to these just-completed state
            // reads. The source event keeps its own immutable server timestamp
            // inside the fetched event but does not age the new observation.
            observed_at_ms: observed_at,
            source: SourceObservation {
                event_id: source
                    .get("event_id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Authority)?
                    .to_owned(),
                room_id: source
                    .get("room_id")
                    .and_then(Value::as_str)
                    .unwrap_or(&input.request.source_room_id)
                    .to_owned(),
                sender: source
                    .get("sender")
                    .and_then(Value::as_str)
                    .ok_or(Error::Authority)?
                    .to_owned(),
                event_type: source
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(Error::Authority)?
                    .to_owned(),
                content: source.get("content").cloned().ok_or(Error::Authority)?,
            },
            reception,
            project,
            owner_room,
        },
    )
    .map_err(|_| Error::Authority)?;
    proof
        .check_fresh(observed_at)
        .map_err(|_| Error::Authority)?;
    let engagement_id = proof
        .request()
        .engagement_id()
        .map_err(|_| Error::Document)?;
    let observation_digest = canonical::digest(&serde_json::json!({
        "observer": observer_identity,
        "agent": agent_identity,
        "request": source,
        "agent_room": agent_room,
        "project_room": project_room,
    }))
    .map_err(|_| Error::State)?;
    let _custody = Repository::open(state).map_err(|_| Error::StateAt("custody_open"))?;
    let mut domain = DomainRepository::open(state).map_err(|_| Error::StateAt("domain_open"))?;
    domain
        .register(&input.registration)
        .map_err(|_| Error::StateAt("registration"))?;
    domain
        .put_resource(&input.resource)
        .map_err(|_| Error::StateAt("resource"))?;
    domain
        .admit(&proof, observed_at)
        .map_err(|_| Error::StateAt("admission"))?;
    domain
        .approve(&format!("adopt_{engagement_id}"), &proof, observed_at)
        .map_err(|_| Error::StateAt("approval"))?;
    let effect_id = format!("provision_{engagement_id}");
    let mut effect = domain
        .effect(&effect_id)
        .map_err(|_| Error::StateAt("effect_read"))?;
    if effect.state == EffectState::Pending {
        effect = domain
            .claim_effect_for(&effect_id)
            .map_err(|_| Error::State)?
            .ok_or(Error::State)?;
    }
    if matches!(effect.state, EffectState::Started | EffectState::Uncertain) {
        domain
            .observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: format!("live_matrix:{observation_digest}"),
                },
            )
            .map_err(|_| Error::State)?;
    } else if effect.state != EffectState::Complete {
        return Err(Error::State);
    }
    let transport = MatrixTransportObservation {
        engagement_id: engagement_id.clone(),
        registration_generation: input.registration.generation,
        generation: input.transport_generation,
        sender_mxid: input.agent_mxid,
        device_id: input.agent_device_id,
    };
    domain
        .observe_matrix_transport(&transport, observed_at)
        .map_err(|_| Error::StateAt("transport"))?;
    domain
        .observe_matrix_room(&agent_room, observed_at)
        .map_err(|_| Error::StateAt("room"))?;
    let binding = SessionBinding {
        id: input.session_id.clone(),
        engagement_id: engagement_id.clone(),
        room_id: input.agent_room_id,
        thread_root: None,
    };
    let resolved = domain
        .resolve_verified_matrix_session(&binding, observed_at)
        .map_err(|_| Error::StateAt("session"))?;
    if resolved.id != binding.id {
        return Err(Error::StateAt("session_identity"));
    }
    domain
        .register_workspace(&input.workspace_id)
        .map_err(|_| Error::StateAt("workspace"))?;
    if let (Some(inbox), Some(room)) = (&input.project_inbox, &project_room) {
        domain
            .observe_matrix_room(room, observed_at)
            .map_err(|_| Error::StateAt("project_room"))?;
        let binding = SessionBinding {
            id: inbox.session_id.clone(),
            engagement_id: engagement_id.clone(),
            room_id: room.room_id.clone(),
            thread_root: None,
        };
        let resolved = domain
            .resolve_verified_matrix_session(&binding, observed_at)
            .map_err(|_| Error::StateAt("project_session"))?;
        if resolved.id != binding.id {
            return Err(Error::StateAt("project_session_identity"));
        }
        domain
            .register_workspace(&inbox.workspace_id)
            .map_err(|_| Error::StateAt("project_workspace"))?;
    }
    Ok(Receipt {
        engagement_id,
        session_id: input.session_id,
        workspace_id: input.workspace_id,
        matrix_observation_digest: observation_digest,
        project_inbox: input.project_inbox,
    })
}
