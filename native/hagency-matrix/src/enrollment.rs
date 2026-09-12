//! Explicit one-pass enrollment on the original Collector and SDK owner.
use crate::{
    CancellationToken, Collector, Error,
    collector::Inner,
    sdk::{
        Owner,
        enrollment::{Command, Handle, Purpose},
    },
};
use state::View;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use tokio::time::Instant;

pub(crate) mod state;
#[derive(Clone, Copy)]
pub(crate) enum Scope<'a> {
    Agent,
    Approval(&'a [String]),
}
impl Scope<'_> {
    fn purpose(self) -> Purpose {
        match self {
            Self::Agent => Purpose::Agent,
            Self::Approval(_) => Purpose::Approval,
        }
    }
}

#[derive(Default)]
pub(crate) struct Jobs(Mutex<Option<Arc<Job>>>);
struct Job {
    result: Mutex<Option<Result<(), Error>>>,
}

impl Collector {
    /// Enroll only the explicitly provisioned ordinary fresh-account profile.
    /// This result creates no dispatch, source, upload or publication authority.
    pub async fn enroll_fresh_account(&self, cancel: &CancellationToken) -> Result<(), Error> {
        if self.inner.config.enrollment.is_none() {
            return Err(Error::Config);
        }
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let deadline = Instant::now() + self.inner.config.limits.sdk;
        let job = Arc::new(Job {
            result: Mutex::new(None),
        });
        {
            let mut registry = self
                .inner
                .enrollment_jobs
                .0
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?;
            if let Some(previous) = registry.as_ref() {
                match *previous.result.lock().map_err(|_| Error::OutcomeUnknown)? {
                    Some(Ok(())) => {}
                    Some(Err(error)) => return Err(error),
                    None => return Err(Error::OutcomeUnknown),
                }
            }
            *registry = Some(job.clone());
        }
        let inner = self.inner.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let expected = inner.expected_transport().await;
            let result = match expected {
                Err(error) => Err(error),
                Ok(expected) => {
                    let result = tokio::time::timeout_at(
                        deadline,
                        inner.enroll(Scope::Agent, &cancel, deadline),
                    )
                    .await
                    .unwrap_or(Err(Error::Timeout));
                    match result {
                        Ok(()) => Ok(()),
                        Err(error) => inner.fence_observation(expected, error).await,
                    }
                }
            };
            if let Ok(mut slot) = job.result.lock() {
                *slot = Some(result);
            }
            result
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
}

impl Inner {
    async fn enrollment_handle(&self, scope: Scope<'_>) -> Result<Handle, Error> {
        let mut guard = self.owner.lock().await;
        if guard.is_none() {
            *guard = Some(Owner::open_existing(&self.config).await?);
        }
        Ok(guard
            .as_ref()
            .ok_or(Error::Storage)?
            .enrollment_handle_for(scope.purpose()))
    }
    async fn enrollment_current(&self, cancel: &CancellationToken) -> Result<Vec<String>, Error> {
        self.expected_transport().await?;
        self.whoami(cancel).await?;
        let mut users = BTreeSet::new();
        for room in &self.config.rooms {
            let observation = self.collect_room_observation(room, cancel).await?;
            users.extend(observation.joined);
            if users.len() > 17 {
                return Err(Error::Capacity);
            }
        }
        let users = users.into_iter().collect::<Vec<_>>();
        self.config
            .enrollment
            .as_ref()
            .ok_or(Error::Config)?
            .users(&self.config.identity.transport.sender_mxid, &users)?;
        let state = self
            .domain
            .matrix_transport_state(self.config.identity.transport.engagement_id.clone())
            .await?;
        if state.is_none_or(|s| !s.available || s.observation != self.config.identity.transport) {
            return Err(Error::Generation);
        }
        Ok(users)
    }
    async fn enrollment_current_for(
        &self,
        scope: Scope<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, Error> {
        match scope {
            Scope::Agent => self.enrollment_current(cancel).await,
            Scope::Approval(engagements) => {
                self.approval_enrollment_current(engagements, cancel).await
            }
        }
    }
    async fn enrollment_query(
        &self,
        owner: &Handle,
        users: &[String],
        cancel: &CancellationToken,
    ) -> Result<serde_json::Value, Error> {
        let View::Query(body) = owner.command(Command::Query(users.to_vec())).await? else {
            return Err(Error::Storage);
        };
        let response = self
            .http
            .post(&["_matrix", "client", "v3", "keys", "query"], body, cancel)
            .await?
            .success()?;
        state::size(&response, state::QUERY)?;
        Ok(response)
    }
    pub(crate) async fn enroll(
        &self,
        scope: Scope<'_>,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<(), Error> {
        checkpoint(cancel, deadline)?;
        let owner = self.enrollment_handle(scope).await?;
        let complete = match owner.command(Command::Status).await? {
            View::Absent => false,
            View::Complete => true,
            _ => return Err(Error::Storage),
        };
        let users = self.enrollment_current_for(scope, cancel).await?;
        let versions = self
            .http
            .request(&["_matrix", "client", "versions"], None, cancel)
            .await?
            .success()?;
        let advertised = versions
            .get("versions")
            .and_then(|v| v.as_array())
            .ok_or(Error::Unsupported)?;
        if advertised.len() > 64
            || !advertised.iter().any(|v| {
                matches!(
                    v.as_str(),
                    Some("v1.11" | "v1.12" | "v1.13" | "v1.14" | "v1.15" | "v1.16" | "v1.17")
                )
            })
        {
            return Err(Error::Unsupported);
        }
        let query = self.enrollment_query(&owner, &users, cancel).await?;
        if complete {
            let View::Complete = owner.command(Command::Verify(query)).await? else {
                return Err(Error::Storage);
            };
            if self.enrollment_current_for(scope, cancel).await? != users {
                return Err(Error::Recipients);
            }
            checkpoint(cancel, deadline)?;
            return Ok(());
        }
        owner.command(Command::Prepare(query)).await?;
        loop {
            checkpoint(cancel, deadline)?;
            match owner.command(Command::Next).await? {
                View::Write(packet) => {
                    owner.command(Command::Possible(packet.index)).await?;
                    let acceptance = owner.acceptance()?;
                    // The last SDK Possible await precedes actual current-token,
                    // room and writer checks immediately before the original POST.
                    if self.enrollment_current_for(scope, cancel).await? != users {
                        return Err(Error::Recipients);
                    }
                    checkpoint(cancel, deadline)?;
                    let response = self
                        .http
                        .post(packet.kind.path(), packet.body, cancel)
                        .await?
                        .success()?;
                    // Transfer the actual completed response directly into the
                    // original SDK command before another cancellation point.
                    acceptance.accept(packet.index, response).await?;
                }
                View::Verify => {
                    if self.enrollment_current_for(scope, cancel).await? != users {
                        return Err(Error::Recipients);
                    }
                    let query = self.enrollment_query(&owner, &users, cancel).await?;
                    owner.command(Command::Verify(query)).await?;
                }
                View::Ready => {
                    if self.enrollment_current_for(scope, cancel).await? != users {
                        return Err(Error::Recipients);
                    }
                    checkpoint(cancel, deadline)?;
                    let View::Complete = owner.command(Command::Finish).await? else {
                        return Err(Error::Storage);
                    };
                    // Finishing persistence is another await, never current readiness proof.
                    if self.enrollment_current_for(scope, cancel).await? != users {
                        return Err(Error::Recipients);
                    }
                    checkpoint(cancel, deadline)?;
                    return Ok(());
                }
                _ => return Err(Error::OutcomeUnknown),
            }
        }
    }
}
pub(crate) fn checkpoint(cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
    if cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else if Instant::now() >= deadline {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/enrollment/mod.rs"]
mod tests;
