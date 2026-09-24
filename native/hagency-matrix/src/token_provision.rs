//! One Host-owned ordinary-account registration step. This is not the inline
//! factory: no engagement completion, room/crypto authority or runtime receipt.
use crate::{
    CancellationToken, Error, HostConfig, HostIdentity, HostRoom, Limits, config::host_endpoint,
    http::Http,
};
use hagency_core::{
    JSON_SAFE_MAX, authority::Registration, canonical, project, replies::MatrixTransportObservation,
};
use hagency_store::{DomainStore, Effect, EffectState};
use reqwest::{Url, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Component, PathBuf},
    sync::Arc,
};
use tokio::time::{Instant, timeout_at};
pub(crate) mod application_service;
mod custody;
mod rooms;
pub use application_service::ApplicationServiceCredential;
use custody::Custody;

const REGISTER: &[&str] = &["_matrix", "client", "v3", "register"];
const WHOAMI: &[&str] = &["_matrix", "client", "v3", "account", "whoami"];

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    application_service: Option<application_service::Profile>,
    registration: RegistrationBinding,
    effect: String,
    fence: u64,
    payload: String,
    engagement: String,
    endpoint: String,
    credential: String,
    user: String,
    device: String,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationBinding {
    fingerprint: String,
    generation: u64,
    server: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedResponse {
    status: u16,
    value: Option<Value>,
}

/// Process Host only: no Deserialize, Debug, Clone or secret accessor. A
/// Started effect supplied by trusted Host code is not network/room evidence.
pub struct TokenAccountProvision {
    /// A restart re-attaching an account this custody already completed. It may
    /// only read: the stored response, then whoami. It can never register or log in.
    reattach: bool,
    pub(crate) factory_rooms: Option<Arc<tokio::sync::Mutex<()>>>,
    as_guard: Option<Arc<application_service::Guard>>,
    scope: Option<ProvisionScope>,
    context: Context,
    endpoint: Url,
    token: String,
    root: PathBuf,
    key: [u8; 32],
    limits: Limits,
    pub(crate) roots: Vec<reqwest::Certificate>,
}
#[derive(Clone)]
struct ProvisionScope {
    domain: DomainStore,
    effect: Effect,
    registration: Registration,
}
struct Configuration<'a> {
    registration: &'a Registration,
    effect: &'a Effect,
    endpoint: &'a str,
    token: &'a str,
    state: PathBuf,
    key: [u8; 32],
    limits: Limits,
    application_service: Option<application_service::Profile>,
}
impl TokenAccountProvision {
    pub fn new(
        registration: &Registration,
        effect: &Effect,
        endpoint: &str,
        token: &str,
        state: PathBuf,
        key: [u8; 32],
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::configured(Configuration {
            registration,
            effect,
            endpoint,
            token,
            state,
            key,
            limits,
            application_service: None,
        })
    }
    pub fn application_service(
        registration: &Registration,
        effect: &Effect,
        endpoint: &str,
        credential: ApplicationServiceCredential,
        state: PathBuf,
        key: [u8; 32],
        limits: Limits,
    ) -> Result<Self, Error> {
        if credential.namespace != format!("{}_", registration.fleet_id) {
            return Err(Error::Config);
        }
        let profile = application_service::Profile {
            namespace: credential.namespace,
            sender: registration.representative_mxid.clone(),
        };
        Self::configured(Configuration {
            registration,
            effect,
            endpoint,
            token: &credential.token,
            state,
            key,
            limits,
            application_service: Some(profile),
        })
    }
    fn configured(config: Configuration<'_>) -> Result<Self, Error> {
        let Configuration {
            registration,
            effect,
            endpoint,
            token,
            state,
            key,
            limits,
            application_service,
        } = config;
        registration.validate().map_err(|_| Error::Config)?;
        limits.validate()?;
        project::identifier(&effect.id, 128).map_err(|_| Error::Config)?;
        let engagement = effect
            .engagement_id
            .strip_prefix("en_")
            .ok_or(Error::Config)?;
        if effect.kind != "provision"
            || effect.state != EffectState::Started
            || effect.fence == 0
            || effect.fence > JSON_SAFE_MAX
            || engagement.len() != 32
            || !engagement
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || if application_service.is_some() {
                !(16..=4096).contains(&token.len())
                    || !token.bytes().all(|b| (33..=126).contains(&b))
            } else {
                token.is_empty()
                    || token.len() > 64
                    || !token.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'=' | b'_' | b'-')
                    })
            }
            || !state.is_absolute()
            || state
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(Error::Config);
        }
        let endpoint = host_endpoint(endpoint)?;
        let user = format!(
            "@{}_{}:{}",
            registration.fleet_id, effect.engagement_id, registration.server_name
        );
        let device = format!("DEVICE_{}", effect.engagement_id);
        hagency_core::replies::matrix_user(&user, &registration.server_name)
            .map_err(|_| Error::Config)?;
        let context = Context {
            application_service,
            registration: RegistrationBinding {
                fingerprint: canonical::digest(
                    &serde_json::to_value(registration).map_err(|_| Error::Config)?,
                )
                .map_err(|_| Error::Config)?,
                generation: registration.generation,
                server: registration.server_name.clone(),
            },
            effect: effect.id.clone(),
            fence: effect.fence,
            payload: canonical::transport_digest(&effect.payload).map_err(|_| Error::Config)?,
            engagement: effect.engagement_id.clone(),
            endpoint: endpoint.as_str().to_owned(),
            credential: project::hash(token.as_bytes()),
            user,
            device,
        };
        Ok(Self {
            reattach: false,
            factory_rooms: None,
            as_guard: None,
            scope: None,
            root: state.join(format!("agent-matrix-{}", effect.id)),
            context,
            endpoint,
            token: token.to_owned(),
            key,
            limits,
            roots: vec![],
        })
    }
    pub fn with_root_pem(mut self, pem: &[u8]) -> Result<Self, Error> {
        if pem.len() > 16384 || self.roots.len() >= 4 {
            return Err(Error::Config);
        }
        self.roots
            .push(reqwest::Certificate::from_pem(pem).map_err(|_| Error::Config)?);
        Ok(self)
    }
    /// `effect` is the original claimed snapshot the store rebuilt for a
    /// provision that is already Complete, so the custody binding is unchanged.
    pub fn for_reattach(mut self) -> Self {
        self.reattach = true;
        self
    }
    pub(crate) fn with_domain(
        mut self,
        domain: DomainStore,
        effect: Effect,
        registration: Registration,
    ) -> Self {
        self.scope = Some(ProvisionScope {
            domain,
            effect,
            registration,
        });
        self
    }
    async fn current(&self, cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
        checkpoint(cancel, deadline)?;
        if let Some(scope) = &self.scope {
            self.validate(scope, deadline).await?;
        }
        if let Some(guard) = &self.as_guard {
            timeout_at(deadline, guard.check(cancel))
                .await
                .map_err(|_| Error::OutcomeUnknown)??;
            if let Some(scope) = &self.scope {
                self.validate(scope, deadline).await?;
            }
        }
        checkpoint(cancel, deadline)
    }
    async fn validate(&self, scope: &ProvisionScope, deadline: Instant) -> Result<(), Error> {
        let (effect, registration) = (scope.effect.clone(), scope.registration.clone());
        if self.reattach {
            timeout_at(
                deadline,
                scope
                    .domain
                    .validate_active_provision_account(effect, registration),
            )
            .await
        } else {
            timeout_at(
                deadline,
                scope
                    .domain
                    .validate_provision_account(effect, registration),
            )
            .await
        }
        .map_err(|_| Error::OutcomeUnknown)??;
        Ok(())
    }
    /// Accepted ownership survives a dropped receiver. The original operation
    /// retains its private lock until network/accepted blocking writes settle.
    /// Reopen never registers again after any WritePossible boundary.
    pub async fn execute(
        self,
        cancel: &CancellationToken,
    ) -> Result<ProvisionedTokenAccount, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let deadline = Instant::now() + self.limits.sdk;
        let cancel = cancel.clone();
        let task = tokio::spawn(async move { self.run_owned(&cancel, deadline).await });
        timeout_at(deadline, task)
            .await
            .map_err(|_| Error::OutcomeUnknown)?
            .map_err(|_| Error::OutcomeUnknown)?
    }
    async fn run_owned(
        mut self,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<ProvisionedTokenAccount, Error> {
        let root = self.root.clone();
        let context = self.context.clone();
        let key = self.key;
        let custody = Arc::new(
            tokio::task::spawn_blocking(move || Custody::open(root, &context, key))
                .await
                .map_err(|_| Error::OutcomeUnknown)??,
        );
        checkpoint(cancel, deadline)?;
        let inspect = custody.clone();
        let records = tokio::task::spawn_blocking(move || inspect.responses())
            .await
            .map_err(|_| Error::OutcomeUnknown)??;
        let complete = records.complete.clone();
        if self.reattach && complete.is_none() {
            // Nothing was completed here: that is a provision, not a re-attach.
            return Err(Error::Storage);
        }
        let response = if self.context.application_service.is_some() {
            self.application_service_run(&custody, records, cancel, deadline)
                .await?
        } else {
            let unauthenticated =
                Http::for_host(&self.endpoint, None, &self.http_limits(), &self.roots)?;
            let mut password = None;
            let initial = if let Some(response) = records.initial {
                response
            } else {
                if records.possible {
                    return Err(Error::OutcomeUnknown);
                }
                let mut bytes = [0u8; 32];
                getrandom::fill(&mut bytes).map_err(|_| Error::Storage)?;
                let secret = format!(
                    "Aa1!{}",
                    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
                );
                bytes.fill(0);
                let body = self.body(&secret, None)?;
                write(&custody, "possible", Value::Null).await?;
                let response = self
                    .post(&unauthenticated, REGISTER, body, cancel, deadline)
                    .await?;
                write(
                    &custody,
                    "initial",
                    serde_json::to_value(&response).map_err(|_| Error::Storage)?,
                )
                .await?;
                password = Some(secret);
                response
            };
            let response = match initial.status {
                200 => {
                    if records.auth_possible || records.auth.is_some() {
                        return Err(Error::Storage);
                    }
                    initial
                }
                401 => {
                    let session = challenge(initial.value.as_ref().ok_or(Error::Wire)?)?;
                    if let Some(response) = records.auth {
                        if !records.auth_possible {
                            return Err(Error::Storage);
                        }
                        response
                    } else {
                        // An earlier 401 is not authority to invent another password
                        // or resume registration after losing the original attempt.
                        let password = password.as_ref().ok_or(Error::OutcomeUnknown)?;
                        if records.auth_possible {
                            return Err(Error::OutcomeUnknown);
                        }
                        let body = self.body(password, Some(session))?;
                        write(&custody, "auth-possible", Value::Null).await?;
                        let response = self
                            .post(&unauthenticated, REGISTER, body, cancel, deadline)
                            .await?;
                        write(
                            &custody,
                            "auth",
                            serde_json::to_value(&response).map_err(|_| Error::Storage)?,
                        )
                        .await?;
                        response
                    }
                }
                403 => return Err(Error::Unauthorized),
                status => return Err(Error::Remote(status)),
            };
            drop(password);
            response
        };
        if response.status != 200 {
            return Err(if matches!(response.status, 401 | 403) {
                Error::Unauthorized
            } else {
                Error::Remote(response.status)
            });
        }
        let value = response.value.as_ref().ok_or(Error::Wire)?;
        let token = credentials(value, &self.context)?.to_owned();
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Wire)?;
        authorization.set_sensitive(true);
        let authenticated = Http::for_host(
            &self.endpoint,
            Some(&authorization),
            &self.http_limits(),
            &self.roots,
        )?;
        checkpoint(cancel, deadline)?;
        let observed = timeout_at(deadline, authenticated.request(WHOAMI, None, cancel))
            .await
            .map_err(|_| Error::OutcomeUnknown)??
            .success()?;
        if observed.get("user_id").and_then(Value::as_str) != Some(self.context.user.as_str())
            || observed.get("device_id").and_then(Value::as_str)
                != Some(self.context.device.as_str())
            || observed
                .get("is_guest")
                .is_some_and(|value| value != &Value::Bool(false))
        {
            return Err(Error::Identity);
        }
        self.current(cancel, deadline).await?;
        let observation = json!({"user_id": self.context.user, "device_id": self.context.device});
        if let Some(previous) = complete {
            if previous != observation {
                return Err(Error::Storage);
            }
        } else {
            write(&custody, "complete", observation).await?;
        }
        checkpoint(cancel, deadline)?;
        Ok(ProvisionedTokenAccount {
            reattach: self.reattach,
            factory_rooms: self.factory_rooms,
            as_guard: self.as_guard,
            scope: self.scope,
            enrollment_jobs: Default::default(),
            room_jobs: Default::default(),
            key: self.key,
            context: self.context,
            token,
            root: self.root,
            limits: self.limits,
            roots: self.roots,
        })
    }
    fn http_limits(&self) -> Limits {
        let mut limits = self.limits.clone();
        limits.bytes = limits.bytes.min(16384);
        limits
    }
    fn body(&self, password: &str, session: Option<&str>) -> Result<String, Error> {
        let mut body = json!({
            "username": self.context.user.split_once(':').ok_or(Error::Config)?.0.trim_start_matches('@'),
            "device_id": self.context.device, "password": password,
            "inhibit_login": false, "refresh_token": false,
        });
        // Submit this credential even on the first call; never retry an
        // unauthenticated/dummy/open-registration alternative after refusal.
        body["auth"] = json!({"type":"m.login.registration_token", "token":self.token});
        if let Some(session) = session {
            body["auth"]["session"] = session.into();
        }
        serde_json::to_string(&body).map_err(|_| Error::Config)
    }
    async fn post(
        &self,
        http: &Http,
        path: &[&str],
        body: String,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<SavedResponse, Error> {
        if self.reattach {
            // The one place a register or login leaves this type.
            return Err(Error::Storage);
        }
        self.current(cancel, deadline).await?;
        checkpoint(cancel, deadline)?;
        let response = timeout_at(deadline, http.post(path, body, cancel))
            .await
            .map_err(|_| Error::OutcomeUnknown)?
            .map_err(|_| Error::OutcomeUnknown)?;
        Ok(SavedResponse {
            status: response.status,
            value: response.value,
        })
    }
}

/// Actual matching register + fresh whoami evidence. Not room, SDK, domain or
/// runtime authority. Secrets stay private and cannot be Debug/serialized.
pub struct ProvisionedTokenAccount {
    /// Re-attached after a restart: every later step only reads and replays.
    reattach: bool,
    factory_rooms: Option<Arc<tokio::sync::Mutex<()>>>,
    as_guard: Option<Arc<application_service::Guard>>,
    scope: Option<ProvisionScope>,
    enrollment_jobs: crate::enrollment::provisioning::Jobs,
    room_jobs: rooms::Jobs,
    key: [u8; 32],
    context: Context,
    token: String,
    root: PathBuf,
    limits: Limits,
    roots: Vec<reqwest::Certificate>,
}
impl ProvisionedTokenAccount {
    pub fn sender_mxid(&self) -> &str {
        &self.context.user
    }
    pub fn device_id(&self) -> &str {
        &self.context.device
    }
    /// Consumes the private credential into the fixed ordinary SDK profile.
    /// Callers must still observe rooms, enroll crypto and launch the runtime.
    pub fn into_host_config(
        self,
        generation: u64,
        key: [u8; 32],
        rooms: Vec<HostRoom>,
    ) -> Result<HostConfig, Error> {
        if self.enrollment_jobs.admitted()? {
            return Err(Error::Busy);
        }
        self.host_config(generation, key, rooms)
    }
    fn host_config(
        &self,
        generation: u64,
        key: [u8; 32],
        rooms: Vec<HostRoom>,
    ) -> Result<HostConfig, Error> {
        let identity = HostIdentity {
            server_name: self.context.registration.server.clone(),
            registration_fingerprint: self.context.registration.fingerprint.clone(),
            transport: MatrixTransportObservation {
                engagement_id: self.context.engagement.clone(),
                registration_generation: self.context.registration.generation,
                generation,
                sender_mxid: self.context.user.clone(),
                device_id: self.context.device.clone(),
            },
        };
        let mut config = HostConfig::new(
            identity,
            &self.context.endpoint,
            &self.token,
            self.root.join("sdk"),
            key,
            rooms,
            self.limits.clone(),
        )?;
        config.factory_rooms = self.factory_rooms.clone();
        config.as_guard = self.as_guard.clone();
        config.roots = self.roots.clone();
        Ok(config)
    }
    /// Enroll the original inline account before activation. Only accounts
    /// retaining that original writer/claim may enter; supplied rooms are
    /// checked against its original request and actual authenticated state.
    /// The returned original Collector is not Active/Applied or a session route.
    pub async fn enroll_before_activation(
        &self,
        generation: u64,
        key: [u8; 32],
        rooms: Vec<HostRoom>,
        anchors: Vec<(String, String)>,
        cancel: &CancellationToken,
    ) -> Result<crate::Collector, Error> {
        self.room_jobs.check_rooms(&rooms)?;
        let original = self.scope.as_ref().ok_or(Error::Config)?;
        let config = self
            .host_config(generation, key, rooms)?
            .with_fresh_account_enrollment(anchors)?;
        let scope = crate::enrollment::provisioning::Scope::new(
            original.effect.clone(),
            original.registration.clone(),
            &config,
        )?;
        self.enrollment_jobs
            .run(config, original.domain.clone(), scope, cancel)
            .await
    }
    /// Re-attach only. Replays this account's completed rooms custody (no
    /// create, invite or join can leave it in this mode), then opens its
    /// existing enrolled SDK store and collects at `generation`.
    pub async fn reattach_collector(
        &self,
        representative_token: &str,
        generation: u64,
        key: [u8; 32],
        anchors: Vec<(String, String)>,
        cancel: &CancellationToken,
    ) -> Result<crate::Collector, Error> {
        if !self.reattach {
            return Err(Error::Config);
        }
        self.create_agent_rooms(representative_token, cancel)
            .await?;
        let rooms = self.room_jobs.rooms()?;
        self.room_jobs.check_rooms(&rooms)?;
        let original = self.scope.as_ref().ok_or(Error::Config)?;
        let config = self
            .host_config(generation, key, rooms)?
            .with_fresh_account_enrollment(anchors)?;
        let scope = crate::enrollment::provisioning::Scope::new(
            original.effect.clone(),
            original.registration.clone(),
            &config,
        )?;
        self.enrollment_jobs
            .reattach_enrolled(config, original.domain.clone(), scope, cancel)
            .await
    }
    /// Close only this retained enrollment SDK. This is not a canonical
    /// transport/route/lifecycle cleanup receipt and cannot rearm enrollment.
    pub async fn close_enrollment_sdk(&self) -> Result<(), Error> {
        self.enrollment_jobs.close_sdk().await
    }
    /// Consume only this account's original enrolled SDK after separately
    /// observed physical Applied. No new configuration, owner, keys or routes.
    pub async fn active_collector(
        &self,
        cancel: &CancellationToken,
    ) -> Result<crate::Collector, Error> {
        self.enrollment_jobs.active(cancel).await
    }
    /// Perform original agent-created DM and representative invite/agent join.
    /// This observes rooms only, never full factory completion or Active state.
    pub async fn create_agent_rooms(
        &self,
        representative_token: &str,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let original = self.scope.as_ref().ok_or(Error::Config)?;
        let operation = rooms::Operation::new(self, original.clone(), representative_token)?;
        self.room_jobs.run(operation, cancel).await
    }
    /// Use only this account's actual created/joined room observations.
    pub async fn enroll_created_rooms(
        &self,
        generation: u64,
        key: [u8; 32],
        anchors: Vec<(String, String)>,
        cancel: &CancellationToken,
    ) -> Result<crate::Collector, Error> {
        let rooms = self.room_jobs.rooms()?;
        self.enroll_before_activation(generation, key, rooms, anchors, cancel)
            .await
    }
    /// Known original DM identity only, not a membership/readiness receipt.
    pub fn created_agent_dm(&self) -> Result<Option<String>, Error> {
        self.room_jobs.dm()
    }
    #[cfg(test)]
    pub(crate) fn observed_active_handoff(&self) -> Option<Result<(), Error>> {
        self.enrollment_jobs.observed_active()
    }
    #[cfg(test)]
    pub(crate) fn observed_enrollment(&self) -> Option<Result<crate::Collector, Error>> {
        self.enrollment_jobs.observed()
    }
}
fn credentials<'a>(value: &'a Value, context: &Context) -> Result<&'a str, Error> {
    if value.get("user_id").and_then(Value::as_str) != Some(context.user.as_str())
        || value.get("device_id").and_then(Value::as_str) != Some(context.device.as_str())
        || value
            .get("home_server")
            .is_some_and(|v| v.as_str() != Some(context.registration.server.as_str()))
    {
        return Err(Error::Identity);
    }
    if value.get("refresh_token").is_some() || value.get("expires_in_ms").is_some() {
        return Err(Error::Unsupported);
    }
    value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| {
            (16..=4096).contains(&token.len()) && token.bytes().all(|b| (33..=126).contains(&b))
        })
        .ok_or(Error::Wire)
}
fn challenge(value: &Value) -> Result<&str, Error> {
    let session = value
        .get("session")
        .and_then(Value::as_str)
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 255
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'=' | b'_' | b'-'))
        })
        .ok_or(Error::Wire)?;
    let flows = value
        .get("flows")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .ok_or(Error::Wire)?;
    let mut token_only = false;
    for flow in flows {
        let stages = flow
            .get("stages")
            .and_then(Value::as_array)
            .filter(|v| !v.is_empty() && v.len() <= 16)
            .ok_or(Error::Wire)?;
        if stages
            .iter()
            .any(|v| v.as_str().is_none_or(|v| v.is_empty() || v.len() > 128))
        {
            return Err(Error::Wire);
        }
        token_only |= stages.as_slice() == [Value::String("m.login.registration_token".into())];
    }
    if value
        .get("completed")
        .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
        || !token_only
    {
        return Err(Error::Unsupported);
    }
    Ok(session)
}
fn checkpoint(cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Error::OutcomeUnknown);
    }
    Ok(())
}
async fn write(custody: &Arc<Custody>, name: &'static str, value: Value) -> Result<(), Error> {
    let custody = custody.clone();
    tokio::task::spawn_blocking(move || custody.write(name, value))
        .await
        .map_err(|_| Error::OutcomeUnknown)?
}
