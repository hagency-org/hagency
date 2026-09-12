//! Original owned-context maintenance, never response or task authority.
use super::*;
use std::{sync::Arc, time::Instant};

pub struct OwnedApprovalScope {
    proof: Proof,
}
pub struct OwnedApprovalStatus {
    pub task: hagency_core::tasks::Task,
    pub parked: bool,
    pub approvals: Vec<ApprovalSummary>,
}

#[derive(Clone)]
pub(crate) struct Proof {
    owner: Arc<()>,
    cap: RunnerCapability,
    expected: String,
    context: String,
    context_digest: String,
    until: Instant,
    expires_at: u64,
}
impl OwnedApprovalScope {
    pub(crate) fn proof(&self) -> Proof {
        self.proof.clone()
    }
}
impl Proof {
    pub(crate) fn weight(&self) -> Value {
        json!([
            self.cap,
            self.expected,
            self.context,
            self.context_digest,
            self.expires_at
        ])
    }
}
fn deadline(until: Instant, expires_at: u64, now: u64) -> Result<(), Error> {
    clock(expires_at)?;
    clock(now)?;
    if now >= expires_at || Instant::now() >= until {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}

impl DomainRepository {
    pub fn bind_owned_approval_context(
        &mut self,
        cap: &RunnerCapability,
        expected: &str,
        input: &HostApprovalContext,
        until: Instant,
        expires_at: u64,
        now: u64,
    ) -> Result<OwnedApprovalScope, Error> {
        self.bind_owned_approval_clock(cap, expected, input, until, expires_at, || Ok(now))
    }
    pub(crate) fn bind_owned_approval_clock(
        &mut self,
        cap: &RunnerCapability,
        expected: &str,
        input: &HostApprovalContext,
        until: Instant,
        expires_at: u64,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<OwnedApprovalScope, Error> {
        let workspace = context_input(input)?;
        if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::RunnerAuthority);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?;
        deadline(until, expires_at, now)?;
        if expires_at - now > 30_000 {
            return Err(Error::Capacity);
        }
        let started = super::super::owned_dispatch::scope(&tx, cap, now, &["started"])?;
        if started.fingerprint() != expected {
            return Err(Error::RunnerAuthority);
        }
        // A new spelling of the context must not refresh an old operation end.
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM approval_contexts WHERE dispatch_id=?1 AND fence=?2)",
            params![cap.dispatch_id, cap.fence],
            |r| r.get(0),
        )?;
        if existing {
            return Err(Error::RunnerAuthority);
        }
        let context = bind_context(&tx, cap, input, workspace, now)?;
        let context_digest = canonical::digest(&json!(context))?;
        deadline(until, expires_at, now)?;
        tx.commit()?;
        Ok(OwnedApprovalScope {
            proof: Proof {
                owner: self.approval_owner.clone(),
                cap: cap.clone(),
                expected: expected.into(),
                context: context.id,
                context_digest,
                until,
                expires_at,
            },
        })
    }

    pub fn maintain_owned_approval(
        &mut self,
        scope: &OwnedApprovalScope,
        now: u64,
    ) -> Result<OwnedApprovalStatus, Error> {
        self.maintain_owned_approval_clock(&scope.proof, || Ok(now))
    }
    pub(crate) fn maintain_owned_approval_clock(
        &mut self,
        proof: &Proof,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<OwnedApprovalStatus, Error> {
        if !Arc::ptr_eq(&self.approval_owner, &proof.owner) {
            return Err(Error::RunnerAuthority);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?;
        deadline(proof.until, proof.expires_at, now)?;
        let current =
            super::super::owned_dispatch::scope(&tx, &proof.cap, now, &["started", "parked"])?;
        if current.fingerprint() != proof.expected {
            return Err(Error::RunnerAuthority);
        }
        let context = context(&tx, &proof.context)?;
        if canonical::digest(&json!(context))? != proof.context_digest {
            return Err(Error::RunnerAuthority);
        }
        authorize(&tx, &proof.cap, &context, now)?;
        let (state, capability_until): (String, u64) = tx.query_row(
            "SELECT state,capability_until FROM runner_dispatches WHERE id=?1 AND fence=?2",
            params![proof.cap.dispatch_id, proof.cap.fence],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut query = tx.prepare("SELECT a.id,a.context_id,a.state,a.expires_at,a.grant_id,COALESCE(r.state,''),COALESCE(r.capability_digest,'') FROM owner_approvals a JOIN approval_contexts c ON c.id=a.context_id LEFT JOIN approval_responses r ON r.request_id=a.id WHERE c.dispatch_id=?1 AND c.fence=?2 ORDER BY a.rowid LIMIT 17")?;
        let rows = query
            .query_map(params![proof.cap.dispatch_id, proof.cap.fence], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > 16 {
            return Err(Error::Capacity);
        }
        let mut approvals = Vec::with_capacity(rows.len());
        let mut expires_at = proof.expires_at.min(capability_until);
        let mut unresolved = false;
        let cap_digest = canonical::digest(&json!(proof.cap))?;
        for (id, context_id, application, expiry, grant, response, response_cap) in rows {
            if context_id != proof.context || (!response.is_empty() && response_cap != cap_digest) {
                return Err(Error::RunnerAuthority);
            }
            let pending =
                ["pending", "decided"].contains(&application.as_str()) && response.is_empty();
            let possible = ["authorized", "response_may_send"].contains(&response.as_str());
            if !pending && !possible && response != "write_accepted" {
                return Err(Error::RunnerAuthority);
            }
            if pending || possible {
                if expiry <= now || (state == "started" && (pending || response == "authorized")) {
                    return Err(Error::RunnerAuthority);
                }
                unresolved = true;
                expires_at = expires_at.min(expiry);
                if let Some(grant) = grant {
                    let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM approval_grants WHERE id=?1 AND revoked=0 AND context_key=?2 AND (mode='always' OR (task_id=?3 AND task_epoch=?4)))", params![grant, grant_context(&context)?, context.task, context.epoch], |r| r.get(0))?;
                    if !valid {
                        return Err(Error::RunnerAuthority);
                    }
                }
            }
            approvals.push(summary(&tx, &id)?);
        }
        if state == "parked" && !unresolved {
            return Err(Error::RunnerAuthority);
        }
        let lease_until = now.saturating_add(5_000).min(expires_at);
        deadline(proof.until, expires_at, now)?;
        tx.execute(
            "UPDATE runner_dispatches SET lease_until=?3 WHERE id=?1 AND fence=?2",
            params![proof.cap.dispatch_id, proof.cap.fence, lease_until],
        )?;
        drop(query);
        tx.commit()?;
        Ok(OwnedApprovalStatus {
            task: current.task().clone(),
            parked: state == "parked",
            approvals,
        })
    }
}
