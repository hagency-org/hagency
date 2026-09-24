//! Evidence from the original host owner. Nothing here releases custody.
use super::{DomainRepository, OwnedDispatchScope, execution, serialize};
use crate::Error;
use hagency_core::{
    canonical,
    tasks::{RunnerCapability, clock},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

impl DomainRepository {
    /// Private lifecycle projection; receipt availability is not resolvability.
    pub fn stopped_dispatches_for_agent(
        &self,
        engagement: &str,
        after: &str,
    ) -> Result<Value, Error> {
        hagency_core::project::identifier(engagement, 128)?;
        if !after.is_empty() {
            hagency_core::project::identifier(after, 128)?;
        }
        if !self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM engagements WHERE id=?1)",
            [engagement],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::NotFound);
        }
        let mut rows:Vec<Value>=self.db.prepare(
            "SELECT d.id,d.session_id,d.task_id,d.fence,s.reason,(EXISTS(SELECT 1 FROM owned_stop_inspections i WHERE i.dispatch_id=d.id AND i.fence=d.fence) OR EXISTS(SELECT 1 FROM agent_fences af WHERE af.dispatch_id=d.id AND af.fence=d.fence AND af.cleared_at IS NULL)) FROM runner_dispatches d JOIN runner_sessions r ON r.id=d.session_id JOIN dispatch_stops s ON s.dispatch_id=d.id AND s.fence=d.fence WHERE r.engagement_id=?1 AND d.id>?2 AND d.state='outcome_unknown' AND s.settled_at IS NULL ORDER BY d.id LIMIT 17"
        )?.query_map(params![engagement,after],|r|Ok(json!({"dispatchId":r.get::<_,String>(0)?,"sessionId":r.get::<_,String>(1)?,"taskId":r.get::<_,Option<String>>(2)?,"fence":r.get::<_,u64>(3)?,"reason":r.get::<_,String>(4)?,"inspectionAvailable":r.get::<_,bool>(5)?})))?.collect::<Result<_,_>>()?;
        let next = (rows.len() > 16).then(|| rows[15]["dispatchId"].clone());
        rows.truncate(16);
        Ok(json!({"engagementId":engagement,"dispatches":rows,"nextAfter":next}))
    }

    /// Original host observation only. A committed operator decision is not a
    /// new runner capability; normal claiming still checks current authority.
    pub fn owned_stop_resolution_recorded(&self, cap: &RunnerCapability) -> Result<bool, Error> {
        hagency_core::project::identifier(&cap.dispatch_id, 128)?;
        let attempt:Option<(String,String)>=self.db.query_row(
            "SELECT runner_id,capability_hash FROM runner_attempts WHERE dispatch_id=?1 AND fence=?2",
            params![cap.dispatch_id,cap.fence],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let (runner, hash) = attempt.ok_or(Error::RunnerAuthority)?;
        if runner != cap.runner_id || !execution::matches_secret(&hash, &cap.secret)? {
            return Err(Error::RunnerAuthority);
        }
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM runner_dispatches d JOIN dispatch_stops s ON s.dispatch_id=d.id AND s.fence=d.fence JOIN owned_stop_inspections i ON i.dispatch_id=s.dispatch_id AND i.fence=s.fence WHERE d.id=?1 AND d.fence=?2 AND d.state='outcome_unknown' AND s.reason='owned_runner_failure' AND s.settled_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM resource_leases WHERE dispatch_id=d.id) AND (EXISTS(SELECT 1 FROM outcome_resolutions r JOIN outcome_inspections t ON t.id=r.inspection_id WHERE r.dispatch_id=d.id AND t.dispatch_id=d.id AND t.fence=d.fence AND t.receipt_digest=i.digest AND t.consumed_at IS NOT NULL) OR EXISTS(SELECT 1 FROM dispatch_recoveries r WHERE r.original_id=d.id AND json_valid(r.evidence) AND json_extract(r.evidence,'$.profile')='stopped-continuation-v1' AND json_extract(r.evidence,'$.fence')=d.fence AND json_extract(r.evidence,'$.inspection_digest')=i.digest)))",
            params![cap.dispatch_id,cap.fence],|r|r.get(0))?)
    }

    /// Host-only: caller must be the original worker after actual full process
    /// stop and retained-root inspection. No runner/console JSON route exists.
    pub fn record_owned_stop_inspection(
        &mut self,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
        inventory: &Value,
        now: u64,
    ) -> Result<String, Error> {
        clock(now)?;
        scope.check_started(cap)?;
        let [workspace] = scope.input().resources.as_slice() else {
            return Err(Error::RunnerAuthority);
        };
        if !workspace.exclusive
            || inventory.get("profile").and_then(Value::as_str)
                != Some("stopped-content-inventory-v1")
            || !inventory.get("root").is_some_and(Value::is_object)
            || inventory
                .get("entries")
                .and_then(Value::as_array)
                .is_none_or(|a| a.len() > 1024)
            || serde_json::to_vec(inventory)?.len() > 1024 * 1024
        {
            return Err(Error::Capacity);
        }
        let config =
            json!({"scope":scope.fingerprint(),"workspace":workspace.id,"inventory":inventory});
        let digest = canonical::digest(&json!([cap.dispatch_id, cap.fence, config]))?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let original:Option<(String,String)>=tx.query_row("SELECT runner_id,capability_hash FROM runner_attempts WHERE dispatch_id=?1 AND fence=?2",params![cap.dispatch_id,cap.fence],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let (runner, hash) = original.ok_or(Error::RunnerAuthority)?;
        if runner != cap.runner_id || !execution::matches_secret(&hash, &cap.secret)? {
            return Err(Error::RunnerAuthority);
        }
        let prior: Option<String> = tx
            .query_row(
                "SELECT digest FROM owned_stop_inspections WHERE dispatch_id=?1 AND fence=?2",
                params![cap.dispatch_id, cap.fence],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = prior {
            return if prior == digest {
                Ok(prior)
            } else {
                Err(Error::Conflict)
            };
        }
        let frozen_input = canonical::encode_payload(&serde_json::to_value(scope.input())?)?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM runner_dispatches d JOIN dispatch_stops s ON s.dispatch_id=d.id AND s.fence=d.fence JOIN resource_leases l ON l.dispatch_id=d.id WHERE d.id=?1 AND d.fence=?2 AND d.state='outcome_unknown' AND d.input=?3 AND s.settled_at IS NULL AND l.resource_id=?4 AND l.exclusive=1 AND (SELECT COUNT(*) FROM resource_leases WHERE dispatch_id=d.id)=1)",params![cap.dispatch_id,cap.fence,frozen_input,workspace.id],|r|r.get(0))?;
        if !valid {
            return Err(Error::State);
        }
        tx.execute("INSERT INTO owned_stop_inspections(dispatch_id,fence,digest,config,observed_at) VALUES(?1,?2,?3,?4,?5)",params![cap.dispatch_id,cap.fence,digest,serialize(&config)?,now])?;
        tx.commit()?;
        Ok(digest)
    }
    /// Private operator read, scoped in the writer before returning any inventory.
    pub fn stopped_dispatch_inspection(&self, engagement: &str, id: &str) -> Result<Value, Error> {
        hagency_core::project::identifier(engagement, 128)?;
        hagency_core::project::identifier(id, 128)?;
        let fence:Option<u64>=self.db.query_row(
            "SELECT d.fence FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id WHERE d.id=?1 AND s.engagement_id=?2",
            params![id,engagement],|r|r.get(0)).optional()?;
        self.owned_stop_inspection(id, fence.ok_or(Error::NotFound)?)?
            .ok_or(Error::NotFound)
    }
    /// Historical operator inspection only. This read grants no resume rights.
    pub fn owned_stop_inspection(&self, id: &str, fence: u64) -> Result<Option<Value>, Error> {
        hagency_core::project::identifier(id, 128)?;
        hagency_core::replies::generation(fence)?;
        let row:Option<(String,String,u64)>=self.db.query_row("SELECT digest,config,observed_at FROM owned_stop_inspections WHERE dispatch_id=?1 AND fence=?2",params![id,fence],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        row.map(|(digest,config,observed_at)|Ok(json!({"dispatch_id":id,"fence":fence,"digest":digest,"observation":serde_json::from_str::<Value>(&config)?,"observed_at":observed_at}))).transpose()
    }
}
