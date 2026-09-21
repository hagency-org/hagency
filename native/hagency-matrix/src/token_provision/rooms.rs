//! Physical agent rooms on the original ordinary-account provision claim.
use super::{ProvisionScope, ProvisionedTokenAccount, SavedResponse};
use crate::{
    CancellationToken, Error, HostRoom, collector::Inner, enrollment::checkpoint, http::Http,
};
use hagency_core::{authority::ProjectRequest, canonical, project, replies::RoomPrivacy};
use hagency_store::EffectOutcome;
use reqwest::header::HeaderValue;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::Semaphore,
    time::{Instant, timeout_at},
};
mod custody;
use custody::Custody;

pub(super) struct Operation {
    scope: ProvisionScope,
    /// A restart re-attaching rooms this custody already completed: replay the
    /// stored responses and read current state; never create, invite or join.
    reattach: bool,
    request: ProjectRequest,
    binding: String,
    root: PathBuf,
    key: [u8; 32],
    agent: Arc<Inner>,
    agent_write: Http,
    representative: Http,
    representative_write: Http,
}
impl Operation {
    pub fn new(
        account: &ProvisionedTokenAccount,
        scope: ProvisionScope,
        token: &str,
    ) -> Result<Self, Error> {
        if !(16..=4096).contains(&token.len()) || !token.bytes().all(|b| (33..=126).contains(&b)) {
            return Err(Error::Config);
        }
        let request: ProjectRequest = serde_json::from_value(
            scope
                .effect
                .payload
                .get("request")
                .ok_or(Error::Config)?
                .clone(),
        )
        .map_err(|_| Error::Config)?;
        request
            .validate(&scope.registration)
            .map_err(|_| Error::Config)?;
        if request.engagement_id().map_err(|_| Error::Config)? != scope.effect.engagement_id {
            return Err(Error::Config);
        }
        let binding = canonical::transport_digest(&json!({"kind":"agent-rooms-v1","account":account.context,
            "request":request.digest().map_err(|_|Error::Config)?,"representative":project::hash(token.as_bytes()),
            "agent_credential":project::hash(account.token.as_bytes()),"key":project::hash(&account.key)})).map_err(|_|Error::Config)?;
        let config = account.host_config(
            1,
            account.key,
            vec![HostRoom {
                room_id: request.target_room_id.clone(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
            }],
        )?;
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Config)?;
        authorization.set_sensitive(true);
        let mut small = config.limits.clone();
        small.bytes = small.bytes.min(16384);
        let agent_write = Http::for_host(
            &config.endpoint,
            Some(&config.authorization),
            &small,
            &config.roots,
        )?;
        let representative = Http::for_host(
            &config.endpoint,
            Some(&authorization),
            &config.limits,
            &config.roots,
        )?;
        let representative_write = Http::for_host(
            &config.endpoint,
            Some(&authorization),
            &small,
            &config.roots,
        )?;
        let root = account
            .root
            .parent()
            .ok_or(Error::Config)?
            .join(format!("agent-rooms-{}", scope.effect.id));
        Ok(Self {
            scope,
            reattach: account.reattach,
            request,
            binding,
            root,
            key: account.key,
            agent: Inner::new(
                config,
                account.scope.as_ref().ok_or(Error::Config)?.domain.clone(),
            )?,
            agent_write,
            representative,
            representative_write,
        })
    }
    async fn writer(&self, cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
        checkpoint(cancel, deadline)?;
        self.validate().await?;
        if let Some(guard) = &self.agent.config.as_guard {
            guard.check(cancel).await?;
            self.validate().await?;
        }
        checkpoint(cancel, deadline)
    }
    async fn validate(&self) -> Result<(), Error> {
        let (effect, registration) = (self.scope.effect.clone(), self.scope.registration.clone());
        if self.reattach {
            self.scope
                .domain
                .validate_active_provision_account(effect, registration)
                .await?;
        } else {
            self.scope
                .domain
                .validate_provision_account(effect, registration)
                .await?;
        }
        Ok(())
    }
    async fn agent_current(
        &self,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.writer(cancel, deadline).await?;
        self.agent.whoami(cancel).await?;
        self.writer(cancel, deadline).await
    }
    async fn project(&self, cancel: &CancellationToken, deadline: Instant) -> Result<Value, Error> {
        self.writer(cancel, deadline).await?;
        let who = self
            .representative
            .request(
                &["_matrix", "client", "v3", "account", "whoami"],
                None,
                cancel,
            )
            .await?
            .success()?;
        if who.get("user_id").and_then(Value::as_str)
            != Some(self.scope.registration.representative_mxid.as_str())
            || who
                .get("device_id")
                .and_then(Value::as_str)
                .is_none_or(|s| s.is_empty() || s.len() > 255 || s.chars().any(char::is_control))
            || who
                .get("is_guest")
                .is_some_and(|v| v != &Value::Bool(false))
        {
            return Err(Error::Identity);
        }
        let value = self
            .representative
            .request(
                &[
                    "_matrix",
                    "client",
                    "v3",
                    "rooms",
                    &self.request.target_room_id,
                    "state",
                ],
                None,
                cancel,
            )
            .await?
            .success()?;
        let target = &self.agent.config.rooms[0];
        let (room, facts) = self.agent.room(target, value.clone())?;
        let binding = facts.binding.as_ref().ok_or(Error::Recipients)?;
        let owner = &self.request.owner_mxid;
        let rep = &self.scope.registration.representative_mxid;
        if !room.joined.contains(owner)
            || !room.joined.contains(rep)
            || facts
                .powers
                .get(owner)
                .copied()
                .unwrap_or(facts.default_power)
                < facts.invite_power
            || facts
                .powers
                .get(rep)
                .copied()
                .unwrap_or(facts.default_power)
                < facts.invite_power
            || binding["v"] != 1
            || binding["purpose"] != "project"
            || binding["authVersion"] != 1
            || binding["fleetId"] != self.request.fleet_id
            || binding["projectId"] != self.request.target_project_id
            || binding["ownerMxid"] != *owner
        {
            return Err(Error::Recipients);
        }
        self.writer(cancel, deadline).await?;
        Ok(value)
    }
    fn member(&self, value: &Value, membership: &str) -> bool {
        value.as_array().is_some_and(|events| {
            events.iter().any(|event| {
                event["type"] == "m.room.member"
                    && event["state_key"] == self.agent.config.identity.transport.sender_mxid
                    && event["content"]["membership"] == membership
            })
        })
    }
    fn dm_id(&self, response: &SavedResponse) -> Result<String, Error> {
        let value = success(response)?;
        let room = value
            .get("room_id")
            .and_then(Value::as_str)
            .ok_or(Error::Wire)?;
        hagency_core::replies::matrix_room(room, &self.scope.registration.server_name)
            .map_err(|_| Error::Wire)?;
        if [
            self.request.target_room_id.as_str(),
            self.request.owner_dm_room_id.as_str(),
            self.scope.registration.reception_room_id.as_str(),
        ]
        .contains(&room)
        {
            return Err(Error::Conflict);
        }
        Ok(room.into())
    }
    fn rooms(&self, dm: &str) -> Vec<HostRoom> {
        vec![
            self.agent.config.rooms[0].clone(),
            HostRoom {
                room_id: dm.into(),
                generation: 1,
                privacy: RoomPrivacy::Direct {
                    human_mxid: self.request.owner_mxid.clone(),
                },
            },
        ]
    }
    async fn joined_dm(
        &self,
        dm: &str,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<bool, Error> {
        self.agent_current(cancel, deadline).await?;
        let value = self
            .agent
            .http
            .request(
                &["_matrix", "client", "v3", "rooms", dm, "state"],
                None,
                cancel,
            )
            .await?
            .success()?;
        let target = self.rooms(dm).pop().ok_or(Error::Config)?;
        let (room, _) = self.agent.room(&target, value.clone())?;
        let sender = &self.agent.config.identity.transport.sender_mxid;
        let owner = &self.request.owner_mxid;
        let events = value.as_array().ok_or(Error::Wire)?;
        let created = events.iter().any(|e| {
            e["type"] == "m.room.create"
                && e["state_key"] == ""
                && e["sender"] == *sender
                && e["content"]["m.federate"] == false
                && e["content"]
                    .get("creator")
                    .is_none_or(|v| v.as_str() == Some(sender.as_str()))
        });
        let invited_history = events.iter().any(|e| {
            e["type"] == "m.room.history_visibility"
                && e["state_key"] == ""
                && e["content"]["history_visibility"] == "invited"
        });
        if !created
            || !invited_history
            || !room.invite_only
            || !room.encrypted
            || !room.joined.contains(sender)
            || events.iter().any(|e| {
                e["type"] == "m.room.member"
                    && e["state_key"] != *sender
                    && e["state_key"] != *owner
            })
        {
            return Err(Error::Recipients);
        }
        let joined = room.joined.len() == 2 && room.joined.contains(owner);
        if !joined
            && !events.iter().any(|e| {
                e["type"] == "m.room.member"
                    && e["state_key"] == *owner
                    && e["content"]["membership"] == "invite"
            })
        {
            return Err(Error::Recipients);
        }
        self.writer(cancel, deadline).await?;
        Ok(joined)
    }
    async fn post(
        &self,
        http: &Http,
        path: &[&str],
        body: Value,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<SavedResponse, Error> {
        if self.reattach {
            // The one place a create, invite or join leaves this operation.
            return Err(Error::Storage);
        }
        self.writer(cancel, deadline).await?;
        let response = http
            .post(
                path,
                serde_json::to_string(&body).map_err(|_| Error::Config)?,
                cancel,
            )
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        Ok(SavedResponse {
            status: response.status,
            value: response.value,
        })
    }
    async fn run(
        &self,
        job: &Job,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Vec<HostRoom>, Error> {
        let root = self.root.clone();
        let binding = self.binding.clone();
        let key = self.key;
        let custody = Arc::new(
            tokio::task::spawn_blocking(move || Custody::open(root, binding, key))
                .await
                .map_err(|_| Error::OutcomeUnknown)??,
        );
        let inspect = custody.clone();
        let records = tokio::task::spawn_blocking(move || inspect.values())
            .await
            .map_err(|_| Error::OutcomeUnknown)??;
        let dm = if records.iter().all(Option::is_some) {
            for index in [0, 2, 4] {
                if records[index].as_ref().is_none_or(|v| !v.is_null()) {
                    return Err(Error::Storage);
                }
            }
            let created: SavedResponse =
                serde_json::from_value(records[1].clone().ok_or(Error::Storage)?)
                    .map_err(|_| Error::Storage)?;
            let invited: SavedResponse =
                serde_json::from_value(records[3].clone().ok_or(Error::Storage)?)
                    .map_err(|_| Error::Storage)?;
            let joined: SavedResponse =
                serde_json::from_value(records[5].clone().ok_or(Error::Storage)?)
                    .map_err(|_| Error::Storage)?;
            let dm = self.dm_id(&created)?;
            if success(&invited)?.as_object().is_none_or(|v| !v.is_empty())
                || success(&joined)?.get("room_id").and_then(Value::as_str)
                    != Some(self.request.target_room_id.as_str())
                || records[6].as_ref()
                    != Some(&json!({"dm":dm,"project":self.request.target_room_id}))
            {
                return Err(Error::Storage);
            }
            dm
        } else {
            if records.iter().any(Option::is_some) {
                return Err(Error::OutcomeUnknown);
            }
            if self.reattach {
                // No rooms were ever completed here: that is a provision.
                return Err(Error::Storage);
            }
            write(&custody, "dm-possible", Value::Null).await?;
            self.agent_current(cancel, deadline).await?;
            self.project(cancel, deadline).await?;
            let response = self.post(&self.agent_write,&["_matrix","client","v3","createRoom"],json!({
                "preset":"private_chat","is_direct":true,"invite":[self.request.owner_mxid],
                "name":self.request.agent_definition.name,"creation_content":{"m.federate":false},
                "initial_state":[{"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}},
                    {"type":"m.room.history_visibility","state_key":"","content":{"history_visibility":"invited"}}]
            }),cancel,deadline).await?;
            write(
                &custody,
                "dm-response",
                serde_json::to_value(&response).map_err(|_| Error::Storage)?,
            )
            .await?;
            let dm = self.dm_id(&response)?;
            *job.dm.lock().map_err(|_| Error::OutcomeUnknown)? = Some(dm.clone());
            write(&custody, "invite-possible", Value::Null).await?;
            self.project(cancel, deadline).await?;
            let response = self
                .post(
                    &self.representative_write,
                    &[
                        "_matrix",
                        "client",
                        "v3",
                        "rooms",
                        &self.request.target_room_id,
                        "invite",
                    ],
                    json!({"user_id":self.agent.config.identity.transport.sender_mxid}),
                    cancel,
                    deadline,
                )
                .await?;
            write(
                &custody,
                "invite-response",
                serde_json::to_value(&response).map_err(|_| Error::Storage)?,
            )
            .await?;
            if success(&response)?
                .as_object()
                .is_none_or(|v| !v.is_empty())
            {
                return Err(Error::Wire);
            }
            write(&custody, "join-possible", Value::Null).await?;
            let invited = self.project(cancel, deadline).await?;
            if !self.member(&invited, "invite") {
                return Err(Error::Recipients);
            }
            self.agent_current(cancel, deadline).await?;
            let response = self
                .post(
                    &self.agent_write,
                    &[
                        "_matrix",
                        "client",
                        "v3",
                        "join",
                        &self.request.target_room_id,
                    ],
                    json!({}),
                    cancel,
                    deadline,
                )
                .await?;
            write(
                &custody,
                "join-response",
                serde_json::to_value(&response).map_err(|_| Error::Storage)?,
            )
            .await?;
            if success(&response)?.get("room_id").and_then(Value::as_str)
                != Some(self.request.target_room_id.as_str())
            {
                return Err(Error::Identity);
            }
            dm
        };
        *job.dm.lock().map_err(|_| Error::OutcomeUnknown)? = Some(dm.clone());
        let project = self.project(cancel, deadline).await?;
        if !self.member(&project, "join") {
            return Err(Error::Recipients);
        }
        loop {
            if self.joined_dm(&dm, cancel, deadline).await? {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            checkpoint(cancel, deadline)?;
        }
        self.writer(cancel, deadline).await?;
        if records[6].is_none() {
            write(
                &custody,
                "complete",
                json!({"dm":dm,"project":self.request.target_room_id}),
            )
            .await?;
        }
        self.writer(cancel, deadline).await?;
        Ok(self.rooms(&dm))
    }
}
#[derive(Default)]
pub(super) struct Jobs(Mutex<Option<Arc<Job>>>);
struct Job {
    operation: Operation,
    busy: Arc<Semaphore>,
    result: Mutex<Option<Result<Vec<HostRoom>, Error>>>,
    dm: Mutex<Option<String>>,
}
impl Jobs {
    pub fn rooms(&self) -> Result<Vec<HostRoom>, Error> {
        let job = self
            .0
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .clone()
            .ok_or(Error::Config)?;
        job.result
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .as_ref()
            .cloned()
            .unwrap_or(Err(Error::Busy))
    }
    pub fn check_rooms(&self, rooms: &[HostRoom]) -> Result<(), Error> {
        if self.0.lock().map_err(|_| Error::OutcomeUnknown)?.is_none() {
            return Ok(());
        }
        let actual = self.rooms()?;
        if canonical::transport_digest(&serde_json::to_value(actual).map_err(|_| Error::Config)?)
            .map_err(|_| Error::Config)?
            != canonical::transport_digest(&serde_json::to_value(rooms).map_err(|_| Error::Config)?)
                .map_err(|_| Error::Config)?
        {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    pub fn dm(&self) -> Result<Option<String>, Error> {
        let job = self.0.lock().map_err(|_| Error::OutcomeUnknown)?.clone();
        match job {
            Some(job) => Ok(job.dm.lock().map_err(|_| Error::OutcomeUnknown)?.clone()),
            None => Ok(None),
        }
    }
    pub async fn run(&self, operation: Operation, cancel: &CancellationToken) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let job = {
            let mut guard = self.0.lock().map_err(|_| Error::OutcomeUnknown)?;
            if let Some(job) = guard.as_ref() {
                if job.operation.binding != operation.binding {
                    return Err(Error::Conflict);
                }
                match job
                    .result
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .as_ref()
                {
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(error.clone()),
                    None => return Err(Error::Busy),
                }
                job.clone()
            } else {
                let job = Arc::new(Job {
                    operation,
                    busy: Arc::new(Semaphore::new(1)),
                    result: Mutex::new(None),
                    dm: Mutex::new(None),
                });
                *guard = Some(job.clone());
                job
            }
        };
        let permit = job
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let deadline = Instant::now() + job.operation.agent.config.limits.sdk;
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = timeout_at(deadline, job.operation.run(&job, &cancel, deadline))
                .await
                .unwrap_or(Err(Error::Timeout));
            let result = if result.is_err()
                && job
                    .operation
                    .scope
                    .domain
                    .observe_effect(
                        job.operation.scope.effect.id.clone(),
                        job.operation.scope.effect.fence,
                        EffectOutcome::Unknown,
                    )
                    .await
                    .is_err()
            {
                Err(Error::OutcomeUnknown)
            } else {
                result
            };
            let response = result.as_ref().map(|_| ()).map_err(Clone::clone);
            *job.result.lock().map_err(|_| Error::OutcomeUnknown)? = Some(result);
            response
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
}
fn success(response: &SavedResponse) -> Result<&Value, Error> {
    if response.status != 200 {
        return Err(if matches!(response.status, 401 | 403) {
            Error::Unauthorized
        } else {
            Error::Remote(response.status)
        });
    }
    response.value.as_ref().ok_or(Error::Wire)
}
async fn write(custody: &Arc<Custody>, stage: &'static str, value: Value) -> Result<(), Error> {
    let custody = custody.clone();
    tokio::task::spawn_blocking(move || custody.write(stage, value))
        .await
        .map_err(|_| Error::OutcomeUnknown)?
}
