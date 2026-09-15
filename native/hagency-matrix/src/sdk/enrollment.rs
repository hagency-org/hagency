use super::{Owner, Sdk};
use crate::{Error, enrollment::state::*};
use matrix_sdk_base::BaseClient;
use matrix_sdk_crypto::{
    OlmMachine, UserIdentity, store::DynCryptoStore, types::requests::AnyOutgoingRequest,
};
use matrix_sdk_store_encryption::StoreCipher;
use ruma::{
    OwnedUserId, TransactionId,
    api::{
        IncomingResponse,
        client::keys::{claim_keys, upload_keys, upload_signatures, upload_signing_keys},
    },
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tokio::sync::oneshot;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Agent,
    Approval,
}
impl Purpose {
    pub(crate) fn matches(self, approval: bool) -> bool {
        (self == Self::Approval) == approval
    }
}

/// Same original queue; this clone creates neither an SDK nor another owner.
pub(crate) struct Handle {
    purpose: Purpose,
    tx: tokio::sync::mpsc::Sender<super::Command>,
    timeout: std::time::Duration,
}
pub(crate) struct Acceptance {
    purpose: Purpose,
    permit: tokio::sync::mpsc::OwnedPermit<super::Command>,
    timeout: std::time::Duration,
}
impl Handle {
    pub(crate) fn acceptance(&self) -> Result<Acceptance, Error> {
        let permit = self
            .tx
            .clone()
            .try_reserve_owned()
            .map_err(|_| Error::Busy)?;
        Ok(Acceptance {
            purpose: self.purpose,
            permit,
            timeout: self.timeout,
        })
    }
    pub(crate) async fn command(&self, command: Command) -> Result<View, Error> {
        validate_command(&command)?;
        let (send, receive) = oneshot::channel();
        self.tx
            .try_send(super::Command::Enrollment(self.purpose, command, send))
            .map_err(|_| Error::Busy)?;
        reply(self.timeout, receive).await
    }
}
impl Acceptance {
    pub(crate) async fn accept(self, index: usize, value: Value) -> Result<View, Error> {
        size(&Some(&value), FIELD)?;
        let (send, receive) = oneshot::channel();
        self.permit.send(super::Command::Enrollment(
            self.purpose,
            Command::Accept(index, value),
            send,
        ));
        reply(self.timeout, receive).await
    }
}
async fn reply(
    timeout: std::time::Duration,
    receive: oneshot::Receiver<Result<View, Error>>,
) -> Result<View, Error> {
    tokio::time::timeout(timeout, receive)
        .await
        .map_err(|_| Error::OutcomeUnknown)?
        .map_err(|_| Error::OutcomeUnknown)?
}
fn validate_command(command: &Command) -> Result<(), Error> {
    match command {
        Command::Prepare(value) | Command::Verify(value) => {
            size(value, QUERY)?;
        }
        Command::Accept(_, value) => {
            size(value, FIELD)?;
        }
        Command::Query(users) if users.len() > 17 => return Err(Error::Capacity),
        _ => {}
    }
    Ok(())
}

pub(crate) enum Command {
    Status,
    Query(Vec<String>),
    Prepare(Value),
    Next,
    Possible(usize),
    Accept(usize, Value),
    Verify(Value),
    Finish,
    #[cfg(test)]
    Fault(u8),
    #[cfg(test)]
    HoldReply(ReplyHold),
}

/// One actual completed SDK command retains its original result and owner lock
/// behind this fixture-only boundary. No production wait or fault exists.
#[cfg(test)]
pub(crate) struct ReplyHold {
    pub target: u8,
    pub reached: oneshot::Sender<()>,
    pub release: oneshot::Receiver<()>,
    pub lose_reply: bool,
}

pub(super) async fn load(
    client: &BaseClient,
    cipher: &StoreCipher,
    marker: Option<&str>,
    binding: &str,
    identity: &str,
    user: &str,
    device: &str,
) -> Result<Option<Ledger>, Error> {
    let bytes = client
        .state_store()
        .get_custom_value(KEY)
        .await
        .map_err(|_| Error::Storage)?;
    match (marker, bytes) {
        (None, None) => Ok(None),
        (Some(marker), Some(bytes)) if bytes.len() <= ENVELOPE => {
            let record: Ledger = cipher.decrypt_value(&bytes).map_err(|_| Error::Storage)?;
            record.validate(binding, identity, user, device)?;
            if record.context.marker()? != marker {
                return Err(Error::Storage);
            }
            Ok(Some(record))
        }
        _ => Err(Error::Storage),
    }
}

impl Owner {
    #[cfg(test)]
    pub(crate) fn enrollment_handle(&self) -> Handle {
        self.enrollment_handle_for(Purpose::Agent)
    }
    pub(crate) fn enrollment_handle_for(&self, purpose: Purpose) -> Handle {
        Handle {
            purpose,
            tx: self.tx.clone(),
            timeout: self.timeout,
        }
    }
    #[cfg(test)]
    pub(crate) async fn enrollment(&self, command: Command) -> Result<View, Error> {
        self.enrollment_handle().command(command).await
    }
}

impl Sdk {
    pub(super) async fn enrollment(
        &mut self,
        purpose: Purpose,
        command: Command,
    ) -> Result<View, Error> {
        #[cfg(test)]
        if let Command::Fault(fault) = command {
            self.enrollment_fault = fault;
            return Ok(View::Unit);
        }
        #[cfg(test)]
        if let Command::HoldReply(hold) = command {
            if self.enrollment_hold.is_some() || !(1..=3).contains(&hold.target) {
                return Err(Error::Conflict);
            }
            self.enrollment_hold = Some(hold);
            return Ok(View::Unit);
        }
        if !purpose.matches(self.approval) || self.enrollment_profile.is_none() {
            return Err(Error::Config);
        }
        if self.enrollment_poisoned
            || (self.approval
                && (self.approval_poisoned
                    || self.journal.approval.is_some()
                    || self.journal.approval_delivery.is_some()))
        {
            return Err(Error::OutcomeUnknown);
        }
        if self
            .enrollment
            .as_ref()
            .is_some_and(|r| Some(&r.context.profile) != self.enrollment_profile.as_ref())
        {
            return Err(Error::Conflict);
        }
        // Clone only the same BaseClient's owner handle, never create another SDK.
        let client = self.client.clone();
        let guard = client.olm_machine().await;
        let machine = guard.as_ref().ok_or(Error::Storage)?;
        match command {
            Command::Status => match &self.enrollment {
                None => Ok(View::Absent),
                Some(record) if record.phase == Phase::Complete => {
                    completed(machine, record).await?;
                    Ok(View::Complete)
                }
                Some(_) => Err(Error::OutcomeUnknown),
            },
            Command::Query(users) => {
                self.enrollment_profile
                    .as_ref()
                    .ok_or(Error::Config)?
                    .users(machine.user_id().as_str(), &users)?;
                if let Some(record) = &self.enrollment
                    && (!matches!(record.phase, Phase::Query | Phase::Complete)
                        || record.users != users)
                {
                    return Err(Error::Conflict);
                }
                let parsed = users
                    .iter()
                    .map(|u| OwnedUserId::try_from(u.as_str()).map_err(|_| Error::Wire))
                    .collect::<Result<Vec<_>, _>>()?;
                let (id, request) = machine.query_keys_for_users(parsed.iter().map(|u| u.as_ref()));
                let body = encode(&json!({"device_keys":request.device_keys}), FIELD)?;
                self.enrollment_query = Some(Query {
                    id: id.to_string(),
                    body: body.clone(),
                    response: None,
                });
                Ok(View::Query(body))
            }
            Command::Prepare(response) => self.prepare_enrollment(machine, response).await,
            Command::Next => {
                let record = self.enrollment.as_ref().ok_or(Error::Storage)?;
                match record.phase {
                    Phase::Query => Ok(View::Verify),
                    Phase::Ready => Ok(View::Ready),
                    Phase::Writing => {
                        let (index, w) = record
                            .writes
                            .iter()
                            .enumerate()
                            .find(|(_, w)| w.phase != WritePhase::Applied)
                            .ok_or(Error::Storage)?;
                        if w.phase != WritePhase::Prepared {
                            return Err(Error::OutcomeUnknown);
                        }
                        Ok(View::Write(Packet {
                            index,
                            kind: w.kind,
                            body: w.body.clone(),
                        }))
                    }
                    _ => Err(Error::OutcomeUnknown),
                }
            }
            Command::Possible(index) => {
                let record = self.enrollment.as_mut().ok_or(Error::Storage)?;
                if record.phase != Phase::Writing
                    || record
                        .writes
                        .iter()
                        .position(|w| w.phase != WritePhase::Applied)
                        != Some(index)
                {
                    return Err(Error::Conflict);
                }
                let write = &mut record.writes[index];
                if write.phase != WritePhase::Prepared {
                    return Err(Error::OutcomeUnknown);
                }
                write.phase = WritePhase::Possible;
                self.persist_enrollment().await?;
                Ok(View::Unit)
            }
            Command::Accept(index, response) => {
                self.accept_enrollment(machine, index, response).await
            }
            Command::Verify(response) => self.verify_enrollment(machine, response).await,
            Command::Finish => {
                if self
                    .enrollment
                    .as_ref()
                    .is_none_or(|r| r.phase != Phase::Ready)
                {
                    return Err(Error::Conflict);
                }
                let record = self.enrollment.as_ref().ok_or(Error::Storage)?;
                completed(machine, record).await?;
                self.enrollment.as_mut().ok_or(Error::Storage)?.phase = Phase::Complete;
                self.persist_enrollment().await?;
                self.enrollment_reservation = None;
                Ok(View::Complete)
            }
            #[cfg(test)]
            Command::Fault(_) | Command::HoldReply(_) => unreachable!(),
        }
    }

    async fn prepare_enrollment(
        &mut self,
        machine: &OlmMachine,
        response: Value,
    ) -> Result<View, Error> {
        if self.enrollment.is_some()
            || self.journal.enrollment.is_some()
            || self.journal.outgoing.is_some()
        {
            return Err(Error::Conflict);
        }
        let store: &DynCryptoStore = std::ops::Deref::deref(machine.store());
        let status = machine.cross_signing_status().await;
        if status.has_master
            || status.has_self_signing
            || status.has_user_signing
            || store
                .load_identity()
                .await
                .map_err(|_| Error::Storage)?
                .is_some()
            || machine
                .get_identity(machine.user_id(), None)
                .await
                .map_err(|_| Error::Storage)?
                .is_some()
        {
            return Err(Error::Identity);
        }
        let query = self.enrollment_query.as_ref().ok_or(Error::Conflict)?;
        let body: Value = serde_json::from_str(&query.body).map_err(|_| Error::Storage)?;
        let users = body["device_keys"]
            .as_object()
            .ok_or(Error::Storage)?
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let parsed = users
            .iter()
            .map(|u| OwnedUserId::try_from(u.as_str()).map_err(|_| Error::Wire))
            .collect::<Result<Vec<_>, _>>()?;
        let profile = self
            .enrollment_profile
            .as_ref()
            .ok_or(Error::Config)?
            .clone();
        super::keys::anchored_initial(machine, &parsed, &query.id, &response, &profile.anchors)
            .await?;
        self.enrollment_reservation = Some(reserve()?);
        let mut initial = self.enrollment_query.take().ok_or(Error::Storage)?;
        initial.response = Some(response);
        size(&initial, QUERY)?;
        let context = Context {
            binding: self.enrollment_binding.clone(),
            identity: self.identity.clone(),
            user: machine.user_id().to_string(),
            device: machine.device_id().to_string(),
            profile,
        };
        self.journal.enrollment = Some(context.marker()?);
        // Persist marker first: a torn bootstrap refuses even when the record is absent.
        if let Err(error) = self.persist().await {
            self.enrollment_poisoned = true;
            return Err(error);
        }
        self.enrollment = Some(Ledger {
            context,
            users,
            phase: Phase::Preparing,
            writes: Vec::new(),
            initial,
            verified: None,
            public: None,
            sessions: Vec::new(),
        });
        self.persist_enrollment().await?;
        #[cfg(test)]
        if self.enrollment_fault == 1 {
            self.enrollment_poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        // Original SDK return stays inside this owner even if request encoding or persistence fails.
        self.enrollment_original = Some(
            machine
                .bootstrap_cross_signing(false)
                .await
                .map_err(|_| Error::OutcomeUnknown)?,
        );
        let bootstrap = self.enrollment_original.as_ref().ok_or(Error::Storage)?;
        let upload = bootstrap
            .upload_keys_req
            .as_ref()
            .ok_or(Error::Unsupported)?;
        let AnyOutgoingRequest::KeysUpload(keys) = upload.request() else {
            return Err(Error::Unsupported);
        };
        if keys.device_keys.is_none() || keys.one_time_keys.is_empty() {
            return Err(Error::Unsupported);
        }
        let signing = &bootstrap.upload_signing_keys_req;
        let writes = vec![
            Write::new(
                Kind::Device,
                upload.request_id().to_string(),
                json!({"device_keys":keys.device_keys,"one_time_keys":keys.one_time_keys,"fallback_keys":keys.fallback_keys}),
            )?,
            Write::new(
                Kind::Signing,
                TransactionId::new().to_string(),
                json!({"master_key":signing.master_key,"self_signing_key":signing.self_signing_key,"user_signing_key":signing.user_signing_key}),
            )?,
            Write::new(
                Kind::Signature,
                TransactionId::new().to_string(),
                json!(bootstrap.upload_signatures_req.signed_keys),
            )?,
        ];
        self.enrollment.as_mut().ok_or(Error::Storage)?.writes = writes;
        self.enrollment.as_mut().ok_or(Error::Storage)?.public =
            Some(public_identity(machine).await?);
        for user in parsed.iter().filter(|u| *u != machine.user_id()) {
            let Some(UserIdentity::Other(identity)) = machine
                .get_identity(user, None)
                .await
                .map_err(|_| Error::Storage)?
            else {
                return Err(Error::Recipients);
            };
            self.enrollment_signatures
                .push(identity.verify().await.map_err(|_| Error::Recipients)?);
            let signature = self.enrollment_signatures.last().ok_or(Error::Storage)?;
            let write = Write::new(
                Kind::Signature,
                TransactionId::new().to_string(),
                json!(signature.signed_keys),
            )?;
            self.enrollment
                .as_mut()
                .ok_or(Error::Storage)?
                .writes
                .push(write);
        }
        self.enrollment.as_mut().ok_or(Error::Storage)?.phase = Phase::Writing;
        self.persist_enrollment().await?;
        self.enrollment_original = None;
        self.enrollment_signatures.clear();
        Ok(View::Unit)
    }

    async fn accept_enrollment(
        &mut self,
        machine: &OlmMachine,
        index: usize,
        response: Value,
    ) -> Result<View, Error> {
        let record = self.enrollment.as_mut().ok_or(Error::Storage)?;
        if record.phase != Phase::Writing
            || record
                .writes
                .iter()
                .position(|w| w.phase != WritePhase::Applied)
                != Some(index)
        {
            return Err(Error::Conflict);
        }
        let write = &mut record.writes[index];
        if write.phase != WritePhase::Possible {
            return Err(Error::Conflict);
        }
        size(&Some(&response), FIELD)?;
        write.response = Some(response);
        write.phase = WritePhase::Response;
        self.persist_enrollment().await?;
        self.enrollment.as_mut().ok_or(Error::Storage)?.writes[index].phase = WritePhase::Applying;
        self.persist_enrollment().await?;
        #[cfg(test)]
        if self.enrollment_fault == 2 {
            self.enrollment_poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        let write = &self.enrollment.as_ref().ok_or(Error::Storage)?.writes[index];
        let value = write.response.as_ref().ok_or(Error::Storage)?;
        if !value.is_object()
            || value
                .get("failures")
                .is_some_and(|v| v.as_object().is_none_or(|m| !m.is_empty()))
        {
            return Err(Error::Recipients);
        }
        let bytes = encode(value, FIELD)?.into_bytes();
        match write.kind {
            Kind::Device => {
                let body: Value = serde_json::from_str(&write.body).map_err(|_| Error::Storage)?;
                let count = body["one_time_keys"]
                    .as_object()
                    .ok_or(Error::Storage)?
                    .len();
                if value["one_time_key_counts"]["signed_curve25519"]
                    .as_u64()
                    .is_none_or(|n| n < count as u64)
                {
                    return Err(Error::Wire);
                }
                let response =
                    upload_keys::v3::Response::try_from_http_response(http::Response::new(bytes))
                        .map_err(|_| Error::Wire)?;
                machine
                    .mark_request_as_sent(write.id.as_str().into(), &response)
                    .await
                    .map_err(|_| Error::OutcomeUnknown)?;
            }
            Kind::Signing => {
                if value.as_object().is_none_or(|m| !m.is_empty()) {
                    return Err(Error::Wire);
                }
                let response = upload_signing_keys::v3::Response::try_from_http_response(
                    http::Response::new(bytes),
                )
                .map_err(|_| Error::Wire)?;
                machine
                    .mark_request_as_sent(
                        write.id.as_str().into(),
                        matrix_sdk_crypto::types::requests::AnyIncomingResponse::SigningKeysUpload(
                            &response,
                        ),
                    )
                    .await
                    .map_err(|_| Error::OutcomeUnknown)?;
            }
            Kind::Signature => {
                let response = upload_signatures::v3::Response::try_from_http_response(
                    http::Response::new(bytes),
                )
                .map_err(|_| Error::Wire)?;
                machine
                    .mark_request_as_sent(write.id.as_str().into(), &response)
                    .await
                    .map_err(|_| Error::OutcomeUnknown)?;
            }
            Kind::Claim => {
                claim_members(&write.body, value)?;
                let response =
                    claim_keys::v3::Response::try_from_http_response(http::Response::new(bytes))
                        .map_err(|_| Error::Wire)?;
                machine
                    .mark_request_as_sent(write.id.as_str().into(), &response)
                    .await
                    .map_err(|_| Error::OutcomeUnknown)?;
                let record = self.enrollment.as_mut().ok_or(Error::Storage)?;
                for session in &mut record.sessions {
                    let actual = session_ids(machine, &session.curve).await?;
                    if actual.is_empty() || (!session.ids.is_empty() && actual != session.ids) {
                        return Err(Error::Recipients);
                    }
                    session.ids = actual;
                }
            }
        }
        let record = self.enrollment.as_mut().ok_or(Error::Storage)?;
        record.writes[index].phase = WritePhase::Applied;
        if index + 1 == record.writes.len() {
            record.phase = if record.writes[index].kind == Kind::Claim {
                Phase::Ready
            } else {
                Phase::Query
            };
        }
        self.persist_enrollment().await?;
        Ok(View::Unit)
    }

    async fn verify_enrollment(
        &mut self,
        machine: &OlmMachine,
        response: Value,
    ) -> Result<View, Error> {
        let record = self.enrollment.as_ref().ok_or(Error::Storage)?;
        if !matches!(record.phase, Phase::Query | Phase::Complete) {
            return Err(Error::Conflict);
        }
        let complete = record.phase == Phase::Complete;
        let mut query = self.enrollment_query.take().ok_or(Error::Conflict)?;
        let users = record
            .users
            .iter()
            .map(|u| OwnedUserId::try_from(u.as_str()).map_err(|_| Error::Wire))
            .collect::<Result<Vec<_>, _>>()?;
        let recipients = super::keys::accept(machine, &users, &query.id, &response).await?;
        check_anchors(record, &response)?;
        if public_identity(machine).await? != *record.public.as_ref().ok_or(Error::Storage)? {
            return Err(Error::Identity);
        }
        if complete {
            let old = record
                .verified
                .as_ref()
                .and_then(|q| q.response.as_ref())
                .ok_or(Error::Storage)?;
            for field in [
                "device_keys",
                "master_keys",
                "self_signing_keys",
                "user_signing_keys",
            ] {
                if old.get(field) != response.get(field) {
                    return Err(Error::Recipients);
                }
            }
            completed(machine, record).await?;
            return Ok(View::Complete);
        }
        query.response = Some(response);
        size(&query, QUERY)?;
        let mut sessions = Vec::new();
        for (user, device) in recipients {
            let parsed: OwnedUserId = user.as_str().try_into().map_err(|_| Error::Wire)?;
            let keys = machine
                .get_device(&parsed, device.as_str().into(), None)
                .await
                .map_err(|_| Error::Storage)?
                .ok_or(Error::Recipients)?;
            let curve = keys.curve25519_key().ok_or(Error::Recipients)?.to_base64();
            let ids = session_ids(machine, &curve).await?;
            sessions.push(Session {
                user,
                device,
                curve,
                before_ids: ids.clone(),
                ids,
            });
        }
        if sessions.is_empty() || sessions.len() > 64 {
            return Err(Error::Recipients);
        }
        let missing = sessions
            .iter()
            .filter(|s| s.ids.is_empty())
            .map(|s| (s.user.clone(), s.device.clone()))
            .collect::<BTreeSet<_>>();
        self.enrollment.as_mut().ok_or(Error::Storage)?.verified = Some(query);
        self.enrollment.as_mut().ok_or(Error::Storage)?.sessions = sessions;
        // The original no-session baseline is durable before session request generation.
        self.persist_enrollment().await?;
        self.enrollment_claim = machine
            .get_missing_sessions(users.iter().map(|u| u.as_ref()))
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        match self.enrollment_claim.as_ref() {
            Some((id, claim)) => {
                let actual = claim
                    .one_time_keys
                    .iter()
                    .flat_map(|(u, ds)| ds.keys().map(move |d| (u.to_string(), d.to_string())))
                    .collect::<BTreeSet<_>>();
                if actual != missing
                    || missing.is_empty()
                    || claim
                        .one_time_keys
                        .values()
                        .any(|ds| ds.values().any(|a| a.as_str() != "signed_curve25519"))
                {
                    return Err(Error::Recipients);
                }
                let body = json!({"one_time_keys":claim.one_time_keys,"timeout":claim.timeout.map(|d|d.as_millis() as u64)});
                let write = Write::new(Kind::Claim, id.to_string(), body)?;
                let record = self.enrollment.as_mut().ok_or(Error::Storage)?;
                record.writes.push(write);
                record.phase = Phase::Writing;
            }
            None if missing.is_empty() => {
                self.enrollment.as_mut().ok_or(Error::Storage)?.phase = Phase::Ready
            }
            None => return Err(Error::Recipients),
        }
        self.persist_enrollment().await?;
        // Keep the actual SDK return across validation, encoding and persistence
        // errors. Only its committed bounded representation replaces this custody.
        self.enrollment_claim = None;
        Ok(View::Unit)
    }
    async fn persist_enrollment(&mut self) -> Result<(), Error> {
        let result = async {
            let record = self.enrollment.as_ref().ok_or(Error::Storage)?;
            record.validate(
                &self.enrollment_binding,
                &self.identity,
                &record.context.user,
                &record.context.device,
            )?;
            let bytes = self
                .cipher
                .encrypt_value(record)
                .map_err(|_| Error::Storage)?;
            if bytes.len() > ENVELOPE {
                return Err(Error::Capacity);
            }
            self.client
                .state_store()
                .set_custom_value(KEY, bytes)
                .await
                .map_err(|_| Error::Storage)?;
            Ok(())
        }
        .await;
        if result.is_err() {
            self.enrollment_poisoned = true;
        }
        result
    }
}

async fn public_identity(machine: &OlmMachine) -> Result<Value, Error> {
    let store: &DynCryptoStore = std::ops::Deref::deref(machine.store());
    let identity = store
        .load_identity()
        .await
        .map_err(|_| Error::Storage)?
        .ok_or(Error::Identity)?;
    if identity.user_id() != machine.user_id() || !identity.status().await.is_complete() {
        return Err(Error::Identity);
    }
    let master = identity.master_public_key().await.ok_or(Error::Identity)?;
    let signing = identity
        .self_signing_public_key()
        .await
        .ok_or(Error::Identity)?;
    let user = identity
        .user_signing_public_key()
        .await
        .ok_or(Error::Identity)?;
    Ok(
        json!({"master":master.as_ref(),"self_signing":signing.as_ref(),"user_signing":user.as_ref()}),
    )
}
async fn session_ids(machine: &OlmMachine, curve: &str) -> Result<Vec<String>, Error> {
    let store: &DynCryptoStore = std::ops::Deref::deref(machine.store());
    let sessions = store
        .get_sessions(curve)
        .await
        .map_err(|_| Error::Storage)?
        .unwrap_or_default();
    if sessions.len() > 4 {
        return Err(Error::Capacity);
    }
    let mut ids = Vec::new();
    for session in sessions {
        if session.sender_key().to_base64() != curve
            || session.algorithm().await.as_str() != "m.olm.v1.curve25519-aes-sha2"
        {
            return Err(Error::Recipients);
        }
        ids.push(session.session_id().to_owned());
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}
pub(super) async fn completed(machine: &OlmMachine, record: &Ledger) -> Result<(), Error> {
    if public_identity(machine).await? != *record.public.as_ref().ok_or(Error::Storage)? {
        return Err(Error::Identity);
    }
    let response = record
        .verified
        .as_ref()
        .and_then(|q| q.response.as_ref())
        .ok_or(Error::Storage)?;
    check_anchors(record, response)?;
    let mut recipients = BTreeSet::new();
    for (user, devices) in response["device_keys"].as_object().ok_or(Error::Storage)? {
        for (device, keys) in devices.as_object().ok_or(Error::Storage)? {
            if user == &record.context.user && device == &record.context.device {
                continue;
            }
            let curve = keys["keys"][format!("curve25519:{device}")]
                .as_str()
                .ok_or(Error::Storage)?;
            recipients.insert((user.as_str(), device.as_str(), curve));
        }
    }
    let retained = record
        .sessions
        .iter()
        .map(|s| (s.user.as_str(), s.device.as_str(), s.curve.as_str()))
        .collect::<BTreeSet<_>>();
    if recipients != retained || retained.len() != record.sessions.len() {
        return Err(Error::Recipients);
    }
    for session in &record.sessions {
        if session.ids.is_empty() || session_ids(machine, &session.curve).await? != session.ids {
            return Err(Error::Recipients);
        }
    }
    Ok(())
}
fn check_anchors(record: &Ledger, response: &Value) -> Result<(), Error> {
    let public = record.public.as_ref().ok_or(Error::Storage)?;
    for (field, kind) in [
        ("master_keys", "master"),
        ("self_signing_keys", "self_signing"),
        ("user_signing_keys", "user_signing"),
    ] {
        if !super::keys::same_fields(
            &response[field][&record.context.user],
            &public[kind],
            &["user_id", "usage", "keys"],
        ) {
            return Err(Error::Identity);
        }
    }
    for user in record.users.iter().filter(|u| **u != record.context.user) {
        let anchor = record
            .context
            .profile
            .anchors
            .get(user)
            .ok_or(Error::Recipients)?;
        if response["master_keys"][user]["keys"] != json!({format!("ed25519:{anchor}"):anchor}) {
            return Err(Error::Recipients);
        }
    }
    Ok(())
}
fn claim_members(body: &str, response: &Value) -> Result<(), Error> {
    let body: Value = serde_json::from_str(body).map_err(|_| Error::Storage)?;
    let expected = body["one_time_keys"].as_object().ok_or(Error::Storage)?;
    let actual = response["one_time_keys"]
        .as_object()
        .ok_or(Error::Recipients)?;
    if expected.keys().collect::<BTreeSet<_>>() != actual.keys().collect::<BTreeSet<_>>() {
        return Err(Error::Recipients);
    }
    for (user, devices) in expected {
        let devices = devices.as_object().ok_or(Error::Storage)?;
        let keys = actual[user].as_object().ok_or(Error::Recipients)?;
        if devices.keys().collect::<BTreeSet<_>>() != keys.keys().collect::<BTreeSet<_>>() {
            return Err(Error::Recipients);
        }
        for value in keys.values() {
            let keys = value.as_object().ok_or(Error::Recipients)?;
            if keys.len() != 1
                || keys.iter().any(|(id, v)| {
                    !id.starts_with("signed_curve25519:")
                        || !v.is_object()
                        || v.get("signatures").is_none()
                })
            {
                return Err(Error::Recipients);
            }
        }
    }
    Ok(())
}
