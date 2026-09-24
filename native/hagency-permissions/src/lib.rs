//! Host-only approval coordination. No HTTP handlers, Matrix sends or model IO.
//! The repository is the sole policy/grant writer; runtime only maps wire types.
use hagency_core::{approvals::*, tasks::RunnerCapability};
use hagency_runtime::codex::{
    RequestId,
    approval::ApprovalRequest,
    session::{self, SessionDriver, Update},
};
use hagency_store::DomainStore;
use std::collections::BTreeMap;
use tokio::io::{AsyncRead, AsyncWrite};

const MAX_PENDING: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("approval coordinator scope or lifecycle is invalid")]
    Scope,
    #[error("approval coordinator capacity is exhausted")]
    Capacity,
    #[error("approval repository rejected the operation: {0}")]
    Store(#[from] hagency_store::Error),
    #[error("approval protocol rejected the operation: {0}")]
    Runtime(#[from] session::Error),
}

/// Host scheduling data, never deserialized from upstream params. The exact
/// thread/turn/cwd/write policy are obtained from the validated SessionDriver;
/// the repository proves current exclusive workspace custody for writable work.
pub struct Binding {
    pub context_id: String,
    pub connection_id: String,
    pub workspace_resource: String,
}

pub enum HostUpdate {
    Approval(ApprovalSummary),
    /// This is callback termination, including cancellation, not Applied.
    Resolution(Option<ApprovalSummary>),
    Session(Update),
}
struct Pending {
    request: ApprovalRequest,
    application: Option<ApprovalApplication>,
}

/// Owns the single typed session, preventing session/connection swaps after
/// binding. The default SessionDriver and OwnedSession do not attach this.
pub struct CodexApprovals<R, W, E> {
    session: SessionDriver<R, W, E>,
    store: DomainStore,
    cap: RunnerCapability,
    context: HostApprovalContext,
    pending: BTreeMap<String, Pending>,
    closed: bool,
}
impl<R, W, E> CodexApprovals<R, W, E> {
    fn close_local(&mut self) {
        self.closed = true;
        self.session.close();
    }
    pub fn outcome(&self) -> Option<&session::Outcome> {
        self.session.outcome()
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}
impl<R, W, E> Drop for CodexApprovals<R, W, E> {
    fn drop(&mut self) {
        self.close_local();
    }
}
struct Operation<'a, R, W, E> {
    coordinator: &'a mut CodexApprovals<R, W, E>,
    finished: bool,
}
impl<R, W, E> Drop for Operation<'_, R, W, E> {
    fn drop(&mut self) {
        // The writer may already have committed Applying when its response is
        // lost. Never resume/re-send after cancellation; reopen retains custody.
        if !self.finished {
            self.coordinator.close_local();
        }
    }
}
impl<R, W, E> Operation<'_, R, W, E> {
    fn finish<T>(mut self, result: Result<T, Error>) -> Result<T, Error> {
        self.finished = true;
        result
    }
}
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin> CodexApprovals<R, W, E> {
    pub async fn attach(
        mut session: SessionDriver<R, W, E>,
        store: DomainStore,
        cap: RunnerCapability,
        binding: Binding,
    ) -> Result<Self, Error> {
        if session.phase() != session::Phase::Running {
            return Err(Error::Scope);
        }
        let context = HostApprovalContext {
            id: binding.context_id,
            connection_id: binding.connection_id,
            thread_id: session.thread_id().ok_or(Error::Scope)?.into(),
            turn_id: session.turn_id().ok_or(Error::Scope)?.into(),
            workspace_resource: binding.workspace_resource,
            workspace: session.settings().cwd().into(),
            windows_paths: cfg!(windows),
            // Multi-environment execution is not configured by this session.
            environment_id: None,
            may_write: !session.settings().is_read_only(),
            yolo: false,
        };
        store
            .bind_approval_context(cap.clone(), context.clone())
            .await?;
        session.enable_approvals()?;
        Ok(Self {
            session,
            store,
            cap,
            context,
            pending: BTreeMap::new(),
            closed: false,
        })
    }
    pub async fn next_update(&mut self, expires_at: u64) -> Result<HostUpdate, Error> {
        if self.closed {
            return Err(Error::Scope);
        }
        let operation = Operation {
            coordinator: self,
            finished: false,
        };
        let result = operation.coordinator.next_inner(expires_at).await;
        if result.is_err()
            && let Err(error) = operation.coordinator.close().await
        {
            return operation.finish(Err(error));
        }
        operation.finish(result)
    }
    async fn next_inner(&mut self, expires_at: u64) -> Result<HostUpdate, Error> {
        match self.session.next_update().await? {
            Update::Approval(request) => {
                if self.pending.len() >= MAX_PENDING {
                    return Err(Error::Capacity);
                }
                let input = HostApprovalRequest {
                    context_id: self.context.id.clone(),
                    upstream_id: rpc_id(request.id())?,
                    item_id: request.item_id().into(),
                    method: request.method().into(),
                    params: request.params().clone(),
                    expires_at,
                };
                let summary = self
                    .store
                    .request_owner_approval(self.cap.clone(), input)
                    .await?;
                self.pending.insert(
                    summary.id.clone(),
                    Pending {
                        request,
                        application: None,
                    },
                );
                Ok(HostUpdate::Approval(summary))
            }
            Update::ApprovalResolved { id } => {
                let matched = self
                    .pending
                    .iter()
                    .find(|(_, p)| p.request.id() == &id)
                    .map(|(id, p)| (id.clone(), p.application.clone()));
                let summary = match matched {
                    Some((_, Some(application))) => Some(self.uncertain(application, "Codex callback resolved before proven core application; cancellation is indistinguishable").await?),
                    Some((id, None)) => {
                        // Cancellation before a decision must never become a
                        // later response. Close this session; pending authority
                        // expires without native application.
                        self.close_local();
                        Some(self.store.approval_summary(id).await?)
                    }
                    None => None,
                };
                Ok(HostUpdate::Resolution(summary))
            }
            update => {
                if matches!(update, Update::TurnEnded) {
                    self.close().await?;
                }
                Ok(HostUpdate::Session(update))
            }
        }
    }
    /// Persist Applying before even constructing response bytes. Successful
    /// return means the host AsyncWrite accepted/flushed; it is not Applied.
    pub async fn apply(&mut self, id: &str) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Scope);
        }
        let pending = self.pending.get(id).ok_or(Error::Scope)?;
        if pending.application.is_some() {
            return Err(Error::Scope);
        }
        let operation = Operation {
            coordinator: self,
            finished: false,
        };
        let result = operation.coordinator.apply_inner(id).await;
        operation.finish(result)
    }
    async fn apply_inner(&mut self, id: &str) -> Result<(), Error> {
        let application = self
            .store
            .consume_owner_approval(self.cap.clone(), id.into())
            .await?;
        let pending = self.pending.get_mut(id).ok_or(Error::Scope)?;
        pending.application = Some(application.clone());
        let request = &pending.request;
        if application.connection_id != self.context.connection_id
            || application.upstream_id != rpc_id(request.id())?
            || application.thread_id != request.thread_id()
            || application.turn_id != request.turn_id()
            || application.item_id != request.item_id()
        {
            self.close_local();
            self.uncertain(
                application,
                "Durable application descriptor differs from the owned connection",
            )
            .await?;
            return Err(Error::Scope);
        }
        let response = request.response(application.allow);
        if let Err(error) = self.session.respond_approval(response).await {
            self.close_local();
            self.uncertain(application, "Typed response transmission failed; accepted bytes do not prove permission application").await?;
            return Err(error.into());
        }
        Ok(())
    }
    async fn uncertain(
        &self,
        application: ApprovalApplication,
        evidence: &str,
    ) -> Result<ApprovalSummary, Error> {
        Ok(self
            .store
            .observe_approval_application(ApprovalApplicationObservation {
                application,
                outcome: ApplicationOutcome::Unknown,
                evidence: evidence.into(),
            })
            .await?)
    }
    /// Explicit host cancellation records uncertainty for consumed requests.
    /// Drop also closes streams; a lost async observation remains Applying and
    /// becomes Uncertain on repository reopen, never permission to retry.
    pub async fn close(&mut self) -> Result<(), Error> {
        self.close_local();
        for pending in self.pending.values() {
            if let Some(application) = &pending.application {
                self.uncertain(
                    application.clone(),
                    "Host closed approval session without effective application evidence",
                )
                .await?;
            }
        }
        Ok(())
    }
}
fn rpc_id(id: &RequestId) -> Result<ApprovalRpcId, Error> {
    // Native JSON-RPC permits signed IDs; schema13 authority deliberately uses
    // JSON-safe nonnegative IDs. Never coerce a negative number into a string.
    let id = match id {
        RequestId::Number(n) => ApprovalRpcId::Number((*n).try_into().map_err(|_| Error::Scope)?),
        RequestId::String(s) => ApprovalRpcId::String(s.clone()),
    };
    id.validate().map_err(|_| Error::Scope)?;
    Ok(id)
}
