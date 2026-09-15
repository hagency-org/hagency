//! Router authority is separate from transmission and native application evidence.
use super::*;
use std::{collections::BTreeSet, sync::Arc, time::Instant};

pub(super) const MAX_PENDING: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResponseState {
    Authorized,
    ResponseMaySend,
    WriteAccepted,
    OutcomeUnknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalResponseObservation {
    /// Host observation of the original local write; never core Applied.
    WriteAccepted,
    OutcomeUnknown,
}
#[derive(Clone, Debug, Serialize)]
pub struct ApprovalResponseSummary {
    pub id: String,
    pub response: ApprovalResponseState,
    pub application: ApplicationOutcome,
    pub write_accepted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Authorized,
    BeginAttempted,
    Admitted,
    WriteObserved,
    Closed,
}

/// Unique original response custody. No public constructor, Clone or serde.
pub struct ApprovalResponseGrant {
    proof: Proof,
    phase: Phase,
    deadline: Option<Instant>,
}
impl ApprovalResponseGrant {
    /// Comparison data only. This projection does not authorize a write.
    pub fn application(&self) -> &ApprovalApplication {
        &self.proof.application
    }
    pub fn is_admitted(&self) -> bool {
        self.phase == Phase::Admitted
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
    pub(crate) fn attempt(grants: &mut [Self], deadline: Instant) -> Result<Vec<Proof>, Error> {
        if grants.is_empty() || grants.len() > MAX_PENDING {
            return Err(Error::Capacity);
        }
        if grants.iter().any(|g| g.phase != Phase::Authorized) {
            return Err(Error::RunnerAuthority);
        }
        let mut ids = BTreeSet::new();
        if grants.iter().any(|g| !ids.insert(&g.proof.application.id)) {
            return Err(Error::RunnerAuthority);
        }
        // Before the first await: cancellation never re-arms this same owner.
        for grant in &mut *grants {
            grant.phase = Phase::BeginAttempted;
            grant.deadline = Some(deadline);
        }
        Ok(grants.iter().map(|g| g.proof.clone()).collect())
    }
    pub(crate) fn acknowledge(grants: &mut [Self]) -> Result<(), Error> {
        for grant in &*grants {
            if grant.phase != Phase::BeginAttempted {
                return Err(Error::RunnerAuthority);
            }
            checked_deadline(grant.deadline.ok_or(Error::RunnerAuthority)?)?;
        }
        for grant in grants {
            grant.phase = Phase::Admitted;
        }
        Ok(())
    }
    pub(crate) fn admitted(&self) -> Result<(Proof, Instant), Error> {
        if self.phase != Phase::Admitted {
            return Err(Error::RunnerAuthority);
        }
        Ok((
            self.proof.clone(),
            self.deadline.ok_or(Error::RunnerAuthority)?,
        ))
    }
    pub(crate) fn check_deadline(&self) -> Result<(), Error> {
        checked_deadline(self.deadline.ok_or(Error::RunnerAuthority)?)
    }
    pub(crate) fn observation(
        &mut self,
        value: ApprovalResponseObservation,
    ) -> Result<Proof, Error> {
        match value {
            ApprovalResponseObservation::WriteAccepted => {
                if !matches!(self.phase, Phase::Admitted | Phase::WriteObserved) {
                    return Err(Error::RunnerAuthority);
                }
                // The original host observed its physical write already. Even
                // a lost receipt cannot return this grant to transmit admission.
                self.phase = Phase::WriteObserved;
            }
            ApprovalResponseObservation::OutcomeUnknown => self.phase = Phase::Closed,
        }
        Ok(self.proof.clone())
    }
}

// Only an original grant can construct this private queue message. The Arc is
// identity of the original repository instance, not reconstructed durable data.
#[derive(Clone)]
pub(crate) struct Proof {
    owner: Arc<()>,
    capability_digest: String,
    context_id: String,
    application: ApprovalApplication,
}
impl Proof {
    pub(crate) fn weight(&self) -> Value {
        json!([self.capability_digest, self.context_id, self.application])
    }
}
fn cap_digest(cap: &RunnerCapability) -> Result<String, Error> {
    super::super::file_delivery::cap_input(cap)?;
    Ok(canonical::digest(&json!(cap))?)
}
fn checked_deadline(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        Err(Error::RunnerAuthority)
    } else {
        Ok(())
    }
}
fn live_decision(
    tx: &Connection,
    cap: &RunnerCapability,
    id: &str,
    now: u64,
) -> Result<Context, Error> {
    let (context, _) = request(tx, id)?;
    authorize(tx, cap, &context, now)?;
    let (choice, grant, expires): (Option<String>, Option<String>, u64) = tx.query_row(
        "SELECT choice,grant_id,expires_at FROM owner_approvals WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if choice.is_none() || expires <= now {
        return Err(Error::RunnerAuthority);
    }
    if let Some(grant) = grant {
        let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM approval_grants WHERE id=?1 AND revoked=0 AND context_key=?2 AND (mode='always' OR (task_id=?3 AND task_epoch=?4)))", params![grant,grant_context(&context)?,context.task,context.epoch], |r|r.get(0))?;
        if !valid {
            return Err(Error::RunnerAuthority);
        }
    }
    Ok(context)
}
fn proof_matches(
    tx: &Connection,
    owner: &Arc<()>,
    cap: &RunnerCapability,
    proof: &Proof,
) -> Result<(), Error> {
    if !Arc::ptr_eq(owner, &proof.owner) || proof.capability_digest != cap_digest(cap)? {
        return Err(Error::RunnerAuthority);
    }
    let matched: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM approval_responses r JOIN owner_approvals a ON a.id=r.request_id WHERE r.request_id=?1 AND r.context_id=?2 AND r.capability_digest=?3 AND r.decision_digest=?4 AND a.application=?5)",params![proof.application.id,proof.context_id,proof.capability_digest,proof.application.digest,serialize(&proof.application)?],|r|r.get(0))?;
    if !matched {
        return Err(Error::RunnerAuthority);
    }
    Ok(())
}
fn barriers(
    tx: &Connection,
    cap: &RunnerCapability,
    context_id: &str,
    now: u64,
) -> Result<Vec<(String, String)>, Error> {
    let mut query=tx.prepare("SELECT a.id,c.id,COALESCE(r.state,'legacy'),COALESCE(r.capability_digest,'') FROM owner_approvals a JOIN approval_contexts c ON c.id=a.context_id LEFT JOIN approval_responses r ON r.request_id=a.id WHERE c.dispatch_id=?1 AND c.fence=?2 ORDER BY a.id LIMIT 17")?;
    let rows = query
        .query_map(params![cap.dispatch_id, cap.fence], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() || rows.len() > MAX_PENDING {
        return Err(Error::Capacity);
    }
    let mut result = Vec::with_capacity(rows.len());
    for (id, context, state, digest) in rows {
        if digest != cap_digest(cap)?
            || context != context_id
            || !["authorized", "response_may_send", "write_accepted"].contains(&state.as_str())
        {
            return Err(Error::RunnerAuthority);
        }
        if state == "write_accepted" {
            // A completed local write closes its request-expiry barrier. Its
            // historical grant cannot extend or rearm a later response.
            let (prior, _) = request(tx, &id)?;
            authorize(tx, cap, &prior, now)?;
        } else {
            live_decision(tx, cap, &id, now)?;
        }
        result.push((id, state));
    }
    Ok(result)
}
fn summary_response(tx: &Connection, id: &str) -> Result<ApprovalResponseSummary, Error> {
    identifier(id, 128)?;
    let (state, application, write_accepted):(String,String,bool)=tx.query_row("SELECT r.state,a.state,r.write_accepted FROM approval_responses r JOIN owner_approvals a ON a.id=r.request_id WHERE r.request_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(Error::NotFound)?;
    Ok(ApprovalResponseSummary {
        id: id.into(),
        write_accepted,
        response: serde_json::from_value(json!(state))?,
        application: match application.as_str() {
            "applied" => ApplicationOutcome::Applied,
            "not_applied" => ApplicationOutcome::NotApplied,
            _ => ApplicationOutcome::Unknown,
        },
    })
}

impl DomainRepository {
    pub fn authorize_approval_response(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        now: u64,
    ) -> Result<ApprovalResponseGrant, Error> {
        self.authorize_approval_response_clock(cap, id, || Ok(now))
    }
    pub(crate) fn authorize_approval_response_clock(
        &mut self,
        cap: &RunnerCapability,
        id: &str,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<ApprovalResponseGrant, Error> {
        identifier(id, 128)?;
        let digest = cap_digest(cap)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?;
        clock(now)?;
        let context = live_decision(&tx, cap, id, now)?;
        let application = consume(&tx, cap, id, now)?;
        tx.execute("INSERT INTO approval_responses(request_id,context_id,capability_digest,decision_digest,state,authorized_at) VALUES(?1,?2,?3,?4,'authorized',?5)",params![id,context.id,digest,application.digest,now])?;
        tx.commit()?;
        Ok(ApprovalResponseGrant {
            proof: Proof {
                owner: self.approval_owner.clone(),
                capability_digest: digest,
                context_id: context.id,
                application,
            },
            phase: Phase::Authorized,
            deadline: None,
        })
    }
    pub fn begin_approval_responses(
        &mut self,
        cap: &RunnerCapability,
        grants: &mut [ApprovalResponseGrant],
        deadline: Instant,
        now: u64,
    ) -> Result<(), Error> {
        let proofs = ApprovalResponseGrant::attempt(grants, deadline)?;
        self.begin_approval_responses_clock(cap, &proofs, deadline, || Ok(now))?;
        ApprovalResponseGrant::acknowledge(grants)?;
        Ok(())
    }
    pub(crate) fn begin_approval_responses_clock(
        &mut self,
        cap: &RunnerCapability,
        proofs: &[Proof],
        deadline: Instant,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<(), Error> {
        if proofs.is_empty() || proofs.len() > MAX_PENDING {
            return Err(Error::Capacity);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?;
        clock(now)?;
        checked_deadline(deadline)?;
        let context = &proofs[0].context_id;
        let mut ids = BTreeSet::new();
        for proof in proofs {
            proof_matches(&tx, &self.approval_owner, cap, proof)?;
            if &proof.context_id != context || !ids.insert(proof.application.id.as_str()) {
                return Err(Error::RunnerAuthority);
            }
        }
        execution::authorize(&tx, cap, now, &["parked"])?;
        let rows = barriers(&tx, cap, context, now)?;
        let expected: BTreeSet<_> = rows
            .iter()
            .filter(|(_, state)| state == "authorized")
            .map(|(id, _)| id.as_str())
            .collect();
        if ids != expected {
            return Err(Error::RunnerAuthority);
        }
        for proof in proofs {
            tx.execute("UPDATE approval_responses SET state='response_may_send',response_started_at=?2 WHERE request_id=?1 AND state='authorized'",params![proof.application.id,now])?;
        }
        tx.execute("UPDATE runner_dispatches SET state='started' WHERE id=?1 AND fence=?2 AND state='parked'",params![cap.dispatch_id,cap.fence])?;
        tx.execute(
            "UPDATE runner_attempts SET outcome='started' WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id, cap.fence],
        )?;
        checked_deadline(deadline)?;
        tx.commit()?;
        Ok(())
    }
    pub fn check_approval_response(
        &mut self,
        cap: &RunnerCapability,
        grant: &ApprovalResponseGrant,
        now: u64,
    ) -> Result<(), Error> {
        let (proof, deadline) = grant.admitted()?;
        self.check_approval_response_clock(cap, &proof, deadline, || Ok(now))
    }
    pub(crate) fn check_approval_response_clock(
        &mut self,
        cap: &RunnerCapability,
        proof: &Proof,
        deadline: Instant,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<(), Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?;
        clock(now)?;
        checked_deadline(deadline)?;
        proof_matches(&tx, &self.approval_owner, cap, proof)?;
        execution::authorize(&tx, cap, now, &["started"])?;
        if barriers(&tx, cap, &proof.context_id, now)?
            .iter()
            .any(|(_, state)| state == "authorized")
        {
            return Err(Error::RunnerAuthority);
        }
        checked_deadline(deadline)?;
        tx.commit()?;
        Ok(())
    }
    pub fn observe_approval_response(
        &mut self,
        grant: &mut ApprovalResponseGrant,
        observation: ApprovalResponseObservation,
    ) -> Result<ApprovalResponseSummary, Error> {
        let proof = grant.observation(observation)?;
        self.observe_approval_response_proof(&proof, observation)
    }
    pub(crate) fn observe_approval_response_proof(
        &mut self,
        proof: &Proof,
        observation: ApprovalResponseObservation,
    ) -> Result<ApprovalResponseSummary, Error> {
        if !Arc::ptr_eq(&self.approval_owner, &proof.owner) {
            return Err(Error::RunnerAuthority);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = summary_response(&tx, &proof.application.id)?;
        let context = context(&tx, &proof.context_id)?;
        let matched:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM approval_responses WHERE request_id=?1 AND context_id=?2 AND capability_digest=?3 AND decision_digest=?4)",params![proof.application.id,proof.context_id,proof.capability_digest,proof.application.digest],|r|r.get(0))?;
        if !matched {
            return Err(Error::RunnerAuthority);
        }
        match observation {
            ApprovalResponseObservation::WriteAccepted => {
                if !matches!(
                    current.response,
                    ApprovalResponseState::ResponseMaySend | ApprovalResponseState::WriteAccepted
                ) {
                    return Err(Error::RunnerAuthority);
                }
                tx.execute(
                    "UPDATE approval_responses SET state='write_accepted',write_accepted=1 WHERE request_id=?1",
                    [&proof.application.id],
                )?;
            }
            ApprovalResponseObservation::OutcomeUnknown => {
                tx.execute(
                    "UPDATE approval_responses SET state='outcome_unknown' WHERE request_id=?1",
                    [&proof.application.id],
                )?;
                // Negative evidence only: fence current continuation and retain leases.
                tx.execute("UPDATE runner_dispatches SET state='parked' WHERE id=?1 AND fence=?2 AND state='started'",params![context.dispatch,context.fence])?;
                tx.execute("UPDATE runner_attempts SET outcome='parked' WHERE dispatch_id=?1 AND fence=?2 AND outcome='started'",params![context.dispatch,context.fence])?;
            }
        }
        let result = summary_response(&tx, &proof.application.id)?;
        tx.commit()?;
        Ok(result)
    }
    /// Historical read only; never reconstructs a grant or admits sending.
    pub fn approval_response_summary(&self, id: &str) -> Result<ApprovalResponseSummary, Error> {
        summary_response(&self.db, id)
    }
}
