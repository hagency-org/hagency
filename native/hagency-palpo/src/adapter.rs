use crate::{Error, HostConfig, Limits, http::Http, wire};
use hagency_core::{JSON_SAFE_MAX, custody::Lane};
use hagency_store::{
    Store,
    outbound::{AckResolution, AckResponse, Command, PublicationResponse, Reply, TransportScope},
};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
#[path = "catalog.rs"]
mod catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Empty,
    AwaitingConsumer,
    Acknowledged,
    Reclaim,
    Published,
    NoPublication,
}

/// One host transport instance. At most one active request per Matrix/work/
/// publication lane; no hidden reverse listener or spawned polling tasks.
pub struct Adapter {
    store: Store,
    registration: hagency_store::outbound::RegistrationIdentity,
    scope: TransportScope,
    http: Http,
    limits: Limits,
    matrix: Mutex<()>,
    work: Mutex<()>,
    publication: Mutex<()>,
}
impl Adapter {
    pub async fn attach(config: HostConfig, store: Store) -> Result<Self, Error> {
        let http = Http::new(&config)?;
        let registration = config.activation.registration.clone();
        let Reply::Scope(scope) = store
            .outbound(Command::Activate(config.activation), now()?)
            .await?
        else {
            return Err(Error::Custody);
        };
        Ok(Self {
            store,
            registration,
            scope,
            http,
            limits: config.limits,
            matrix: Mutex::new(()),
            work: Mutex::new(()),
            publication: Mutex::new(()),
        })
    }

    /// Trusted host consumer uses this with the custody Store's Claim/Start/
    /// Complete/Inspect operations. This scope is not canonical domain authority.
    pub fn scope(&self) -> TransportScope {
        self.scope.clone()
    }

    pub async fn poll_once(&self, lane: Lane, cancel: &CancellationToken) -> Result<Step, Error> {
        let _guard = match lane {
            Lane::Matrix => &self.matrix,
            Lane::Work => &self.work,
        }
        .try_lock()
        .map_err(|_| Error::Busy)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let Reply::Head(unconfirmed) = self
            .command(Command::AckHead {
                scope: self.scope(),
                lane,
            })
            .await?
        else {
            return Err(Error::Custody);
        };
        if let Some(view) = unconfirmed {
            return self.ack(lane, view.receipt.id, cancel).await;
        }
        if lane == Lane::Matrix {
            let Reply::Head(head) = self
                .command(Command::Head {
                    scope: self.scope(),
                    lane,
                })
                .await?
            else {
                return Err(Error::Custody);
            };
            if head.is_some_and(|v| v.lease_state != "stale") {
                return Ok(Step::AwaitingConsumer);
            }
        }
        let Reply::Poll(poll) = self
            .command(Command::BeginPoll {
                scope: self.scope(),
                lane,
            })
            .await?
        else {
            return Err(Error::Custody);
        };
        let wait = self.limits.poll_wait.as_millis().to_string();
        let query = [
            ("lane", lane.as_str()),
            ("consumer", poll.consumer()),
            ("wait", wait.as_str()),
        ];
        let body = self
            .http
            .request("poll", Some(&query), None, cancel)
            .await?
            .success()?;
        let Some(delivery) = wire::poll(body, self.scope.machine_generation(), lane)? else {
            return Ok(Step::Empty);
        };
        let id = delivery.id.clone();
        // No cancellation select here: a validated response must commit before
        // we can stop or ACK. Writer timeout is explicitly OutcomeUnknown.
        let Reply::Received(_) = self.command(Command::Receive { poll, delivery }).await? else {
            return Err(Error::Custody);
        };
        self.ack(lane, id, cancel).await
    }

    async fn ack(&self, lane: Lane, id: String, cancel: &CancellationToken) -> Result<Step, Error> {
        let Reply::AckTicket(ticket) = self
            .command(Command::BeginAck {
                scope: self.scope(),
                lane,
                id,
            })
            .await?
        else {
            return Err(Error::Custody);
        };
        let body = serde_json::to_string(&ticket.wire_body()).map_err(|_| Error::Wire)?;
        // BeginAck made external uncertainty durable BEFORE sending any bytes.
        let response = self.http.request("ack", None, Some(body), cancel).await?;
        let verdict =
            if response.status == 409 && response.value.as_ref().is_some_and(wire::stale_lease) {
                AckResponse::StaleLease
            } else {
                if !wire::accepted(&response.success()?) {
                    return Err(Error::Wire);
                }
                AckResponse::Accepted
            };
        match self
            .command(Command::Ack {
                ticket,
                response: verdict,
            })
            .await?
        {
            Reply::Ack(AckResolution::Accepted) => Ok(Step::Acknowledged),
            Reply::Ack(AckResolution::Reclaim) => Ok(Step::Reclaim),
            _ => Err(Error::Generation),
        }
    }

    /// Freezes host observations without rewriting timestamps. A pending body
    /// must be reconciled before different content can replace it.
    pub async fn freeze_update(&self, body: Value) -> Result<(), Error> {
        let _guard = self.publication.try_lock().map_err(|_| Error::Busy)?;
        match self
            .command(Command::FreezePublication {
                scope: self.scope(),
                body,
            })
            .await?
        {
            Reply::Publication(Some(_)) => Ok(()),
            _ => Err(Error::Custody),
        }
    }
    pub async fn publish_once(&self, cancel: &CancellationToken) -> Result<Step, Error> {
        let _guard = self.publication.try_lock().map_err(|_| Error::Busy)?;
        self.publish(cancel).await
    }
    async fn publish(&self, cancel: &CancellationToken) -> Result<Step, Error> {
        self.publish_checked(None, cancel).await
    }
    async fn publish_checked(
        &self,
        domain: Option<&hagency_store::DomainStore>,
        cancel: &CancellationToken,
    ) -> Result<Step, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let Reply::Publication(ticket) = self
            .command(Command::PendingPublication(self.scope()))
            .await?
        else {
            return Err(Error::Custody);
        };
        let Some(ticket) = ticket else {
            return Ok(Step::NoPublication);
        };
        self.command(Command::BeginPublication(ticket.clone()))
            .await?;
        if let Some(domain) = domain {
            // Custody writes may wait behind unrelated work. Recheck after
            // those waits, immediately before admitting this HTTP request.
            // Bytes in an already admitted request cannot be recalled.
            domain
                .check_publication_registration(self.registration.clone())
                .await?;
        }
        let value = self
            .http
            .request("updates", None, Some(ticket.wire_body().into()), cancel)
            .await?
            .success()?;
        if !wire::accepted(&value) {
            return Err(Error::Wire);
        }
        self.command(Command::Publication {
            ticket,
            response: PublicationResponse::Accepted,
        })
        .await?;
        Ok(Step::Published)
    }

    /// Three joined loops; cancellation waits for already received custody/known
    /// receipts to commit. Fatal failure cancels siblings cooperatively first.
    pub async fn run(&self, cancel: &CancellationToken) -> Result<(), Error> {
        self.run_source(None, cancel).await
    }

    /// Publish current canonical resources alongside the original inbound lanes.
    /// This host-only opt-in is not a Matrix readiness or execution permission.
    pub async fn run_with_resources(
        &self,
        domain: &hagency_store::DomainStore,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        self.run_source(Some(domain), cancel).await
    }

    async fn run_source(
        &self,
        domain: Option<&hagency_store::DomainStore>,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let child = cancel.child_token();
        let wrap = |result: Result<(), Error>| {
            if result.is_err() {
                child.cancel();
            }
            result
        };
        let (matrix, work, publication) = tokio::join!(
            async { wrap(self.lane_loop(Lane::Matrix, &child).await) },
            async { wrap(self.lane_loop(Lane::Work, &child).await) },
            async { wrap(self.publication_loop(domain, &child).await) },
        );
        for result in [matrix, work, publication] {
            if let Err(error) = result
                && error != Error::Cancelled
            {
                return Err(error);
            }
        }
        Ok(())
    }
    async fn lane_loop(&self, lane: Lane, cancel: &CancellationToken) -> Result<(), Error> {
        let mut delay = self.limits.retry_min;
        loop {
            let result = self.poll_once(lane, cancel).await;
            let wait = match result {
                Ok(_) => {
                    delay = self.limits.retry_min;
                    self.limits.idle
                }
                Err(error) if error.retryable() => backoff(&mut delay, self.limits.retry_max),
                Err(error) => return Err(error),
            };
            pause(cancel, wait).await?;
        }
    }
    async fn publication_loop(
        &self,
        domain: Option<&hagency_store::DomainStore>,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let mut delay = self.limits.retry_min;
        loop {
            // A heartbeat is transport liveness only. No probe, status freshness
            // or authenticated Matrix connection proof is invented here.
            let result = if let Some(domain) = domain {
                self.publish_resources_once(domain, cancel).await
            } else {
                async {
                    let _guard = self.publication.try_lock().map_err(|_| Error::Busy)?;
                    let Reply::Publication(pending) = self
                        .command(Command::PendingPublication(self.scope()))
                        .await?
                    else {
                        return Err(Error::Custody);
                    };
                    if pending.is_none() {
                        self.command(Command::FreezePublication {
                            scope: self.scope(),
                            body: serde_json::json!({"heartbeat":true}),
                        })
                        .await?;
                    }
                    self.publish(cancel).await
                }
                .await
            };
            let wait = match result {
                Ok(_) => {
                    delay = self.limits.retry_min;
                    self.limits.publication_interval
                }
                Err(error) if error.retryable() => backoff(&mut delay, self.limits.retry_max),
                Err(error) => return Err(error),
            };
            pause(cancel, wait).await?;
        }
    }
    async fn command(&self, command: Command) -> Result<Reply, Error> {
        self.store
            .outbound(command, now()?)
            .await
            .map_err(Into::into)
    }
}
fn now() -> Result<u64, Error> {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Config)?
        .as_millis();
    let n = u64::try_from(n).map_err(|_| Error::Config)?;
    if n > JSON_SAFE_MAX {
        return Err(Error::Config);
    }
    Ok(n)
}
fn backoff(delay: &mut Duration, max: Duration) -> Duration {
    let current = *delay;
    *delay = delay.saturating_mul(2).min(max);
    current
}
async fn pause(cancel: &CancellationToken, delay: Duration) -> Result<(), Error> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Cancelled),
        _ = tokio::time::sleep(delay) => Ok(()),
    }
}
