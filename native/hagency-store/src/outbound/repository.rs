use super::*;
use crate::Repository;
use hagency_core::custody::CustodyState;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

fn random_key() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn consumer() -> Result<String, Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    bytes[6] = (bytes[6] & 15) | 64;
    bytes[8] = (bytes[8] & 63) | 128;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}
fn scope(tx: &Transaction<'_>, value: &TransportScope) -> Result<(), Error> {
    let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outbound_transports WHERE binding=?1 AND machine_generation=?2 AND scope=?3 AND consumer=?4)",params![value.binding,value.generation,value.key,value.consumer],|r|r.get(0))?;
    if current {
        Ok(())
    } else {
        Err(Error::Generation)
    }
}
pub(crate) fn retained_bytes(tx: &Transaction<'_>) -> Result<i64, Error> {
    Ok(tx.query_row("SELECT (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))),0) FROM inbox)+(SELECT COALESCE(SUM(length(CAST(result AS BLOB))),0) FROM outbound_attempts)+(SELECT COALESCE(SUM(length(CAST(body AS BLOB))),0) FROM outbound_publications)",[],|r|r.get(0))?)
}
fn capacity(tx: &Transaction<'_>, limit: i64) -> Result<(), Error> {
    if retained_bytes(tx)? > limit {
        Err(Error::Capacity)
    } else {
        Ok(())
    }
}
fn kind(value: &str) -> Result<Kind, Error> {
    Ok(serde_json::from_value(Value::String(value.into()))?)
}
fn kind_text(value: Kind) -> &'static str {
    match value {
        Kind::Transaction => "transaction",
        Kind::Request => "request",
        Kind::Probe => "probe",
    }
}

impl Repository {
    /// Reopening cannot prove whether started adapter work returned. Unstarted
    /// claims are safe to retire, but started work always requires inspection.
    pub(crate) fn recover_outbound(&mut self) -> Result<(), Error> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        recover(&tx, None)?;
        tx.commit()?;
        Ok(())
    }
    pub fn outbound(&mut self, command: Command, now: u64) -> Result<Reply, Error> {
        command.input_bytes()?;
        if now > JSON_SAFE_MAX {
            return Err(InvalidInput("invalid host clock").into());
        }
        // Expiry persists even when the following stale command is refused.
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        recover(&tx, Some(now))?;
        tx.commit()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = match command {
            Command::Activate(a) => activate(&tx, a, self.max_records)?,
            Command::BeginPoll { scope: s, lane } => {
                scope(&tx, &s)?;
                let key = random_key()?;
                tx.execute("INSERT INTO outbound_polls(binding,lane,ticket) VALUES(?1,?2,?3) ON CONFLICT(binding,lane) DO UPDATE SET ticket=excluded.ticket,response_digest=NULL",params![s.binding,lane.as_str(),key])?;
                Reply::Poll(PollTicket {
                    scope: s,
                    lane,
                    key,
                })
            }
            Command::Receive { poll, delivery } => receive(
                &tx,
                poll,
                delivery,
                now,
                self.max_records,
                self.max_payload_bytes,
            )?,
            Command::BeginAck { scope: s, lane, id } => {
                scope(&tx, &s)?;
                let row:Option<(u64,String,String)>=tx.query_row("SELECT lease_generation,lease_token,lease_state FROM inbox WHERE binding=?1 AND lane=?2 AND id=?3",params![s.binding,lane.as_str(),id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
                let Some((generation, token, state)) = row else {
                    return Err(Error::NotFound);
                };
                if generation != s.generation || state == "retired" || state == "stale" {
                    return Err(Error::State);
                }
                tx.execute("UPDATE inbox SET lease_state='unknown' WHERE binding=?1 AND lane=?2 AND id=?3 AND lease_state!='accepted'",params![s.binding,lane.as_str(),id])?;
                Reply::AckTicket(AckTicket {
                    scope: s,
                    lane,
                    id,
                    token,
                })
            }
            Command::Ack { ticket, response } => ack(&tx, ticket, response)?,
            Command::Claim {
                scope: s,
                lane,
                id,
                lease_ms,
            } => claim(&tx, s, lane, id, lease_ms, now, self.max_attempts)?,
            Command::Start(ticket) => start(&tx, ticket, now)?,
            Command::ProcessingUnknown(ticket) => {
                let state:Option<String>=tx.query_row("SELECT state FROM outbound_attempts WHERE binding=?1 AND id=?2 AND capability=?3",params![ticket.binding,ticket.id,ticket.key],|r|r.get(0)).optional()?;
                if !state.is_some_and(|state| ["started", "unknown"].contains(&state.as_str())) {
                    return Err(Error::State);
                }
                tx.execute(
                    "UPDATE outbound_attempts SET state='unknown' WHERE binding=?1 AND id=?2",
                    params![ticket.binding, ticket.id],
                )?;
                tx.execute("UPDATE inbox SET processing_state='unknown' WHERE (binding,lane,id) IN (SELECT binding,lane,delivery_id FROM outbound_attempts WHERE binding=?1 AND id=?2)",params![ticket.binding,ticket.id])?;
                Reply::ProcessingRecorded
            }
            Command::Complete { ticket, result } => {
                let digest = finish(&tx, &ticket, &result, false)?;
                capacity(&tx, self.max_payload_bytes)?;
                Reply::Finished { digest }
            }
            Command::Inspect {
                scope: s,
                attempt_id,
                outcome,
            } => {
                scope(&tx, &s)?;
                let row: Option<(String, String)> = tx
                    .query_row(
                        "SELECT capability,state FROM outbound_attempts WHERE binding=?1 AND id=?2",
                        params![s.binding, attempt_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let Some((key, state)) = row else {
                    return Err(Error::NotFound);
                };
                let ticket = ClaimTicket {
                    binding: s.binding,
                    id: attempt_id,
                    key,
                };
                match outcome {
                    Inspection::Completed(result) => {
                        finish(&tx, &ticket, &result, true)?;
                        capacity(&tx, self.max_payload_bytes)?;
                    }
                    Inspection::Retry => {
                        if state != "retry" {
                            if state != "unknown" {
                                return Err(Error::State);
                            }
                            tx.execute("UPDATE inbox SET processing_state='pending' WHERE (binding,lane,id) IN (SELECT binding,lane,delivery_id FROM outbound_attempts WHERE binding=?1 AND id=?2)",params![ticket.binding,ticket.id])?;
                            tx.execute("UPDATE outbound_attempts SET state='retry' WHERE binding=?1 AND id=?2",params![ticket.binding,ticket.id])?;
                        }
                    }
                }
                Reply::Inspected
            }
            Command::View { scope: s, lane, id } => {
                scope(&tx, &s)?;
                Reply::View(view(&tx, &s, lane, &id)?)
            }
            Command::Head { scope: s, lane } => {
                scope(&tx, &s)?;
                let id:Option<String>=tx.query_row("SELECT id FROM inbox WHERE binding=?1 AND lane=?2 AND processing_state NOT IN ('done','retired') ORDER BY rowid LIMIT 1",params![s.binding,lane.as_str()],|r|r.get(0)).optional()?;
                Reply::Head(id.map(|id| view(&tx, &s, lane, &id)).transpose()?)
            }
            Command::AckHead { scope: s, lane } => {
                scope(&tx, &s)?;
                let id:Option<String>=tx.query_row("SELECT id FROM inbox WHERE binding=?1 AND lane=?2 AND lease_generation=?3 AND lease_state IN ('received','unknown') ORDER BY rowid LIMIT 1",params![s.binding,lane.as_str(),s.generation],|r|r.get(0)).optional()?;
                Reply::Head(id.map(|id| view(&tx, &s, lane, &id)).transpose()?)
            }
            Command::FreezePublication { scope: s, body } => {
                freeze(&tx, s, body, self.max_payload_bytes)?
            }
            Command::PendingPublication(s) => {
                scope(&tx, &s)?;
                Reply::Publication(publication(&tx, &s)?)
            }
            Command::BeginPublication(ticket) => {
                scope(&tx, &ticket.scope)?;
                publication_matches(&tx, &ticket)?;
                tx.execute(
                    "UPDATE outbound_publications SET state='unknown' WHERE binding=?1",
                    [&ticket.scope.binding],
                )?;
                Reply::Publication(Some(ticket))
            }
            Command::Publication { ticket, response } => publication_result(&tx, ticket, response)?,
        };
        tx.commit()?;
        Ok(result)
    }
}
fn view(
    tx: &Transaction<'_>,
    s: &TransportScope,
    lane: Lane,
    id: &str,
) -> Result<DeliveryView, Error> {
    let (receipt,origin,lease,processing):(String,u64,String,String)=tx.query_row("SELECT receipt,origin_machine_generation,lease_state,processing_state FROM inbox WHERE binding=?1 AND lane=?2 AND id=?3",params![s.binding,lane.as_str(),id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(Error::NotFound)?;
    let attempt:Option<(String,Option<String>)>=tx.query_row("SELECT id,result_digest FROM outbound_attempts WHERE binding=?1 AND lane=?2 AND delivery_id=?3 ORDER BY rowid DESC LIMIT 1",params![s.binding,lane.as_str(),id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    Ok(DeliveryView {
        receipt: serde_json::from_str(&receipt)?,
        origin_machine_generation: origin,
        lease_state: lease,
        processing_state: processing,
        attempt_id: attempt.as_ref().map(|(id, _)| id.clone()),
        result_digest: attempt.and_then(|(_, digest)| digest),
    })
}
fn recover(tx: &Transaction<'_>, now: Option<u64>) -> Result<(), Error> {
    tx.execute("UPDATE inbox SET processing_state=CASE WHEN processing_state='claimed' THEN 'pending' ELSE 'unknown' END WHERE (binding,lane,id) IN (SELECT binding,lane,delivery_id FROM outbound_attempts WHERE state IN ('claimed','started') AND (?1 IS NULL OR deadline<=?1))",[now])?;
    tx.execute("UPDATE outbound_attempts SET state=CASE WHEN state='claimed' THEN 'retry' ELSE 'unknown' END WHERE state IN ('claimed','started') AND (?1 IS NULL OR deadline<=?1)",[now])?;
    Ok(())
}
fn activate(tx: &Transaction<'_>, a: Activation, max: i64) -> Result<Reply, Error> {
    let r = &a.registration;
    let identity = serde_json::to_string(r)?;
    let prior:Option<(String,u64,String,String,String)>=tx.query_row("SELECT identity,machine_generation,fingerprint,scope,consumer FROM outbound_transports WHERE binding=?1",[&r.binding],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional()?;
    let (key, consumer) = if let Some((saved, generation, fingerprint, key, consumer)) = prior {
        if saved != identity || a.machine_generation < generation {
            return Err(Error::Generation);
        }
        if a.machine_generation == generation {
            if fingerprint != a.credential_fingerprint {
                return Err(Error::Conflict);
            }
            (key, consumer)
        } else {
            let key = random_key()?;
            // Retired is not a successful remote ACK. Already owned input remains
            // processable because Matrix registration custody has not changed.
            tx.execute("UPDATE inbox SET lease_state='retired' WHERE binding=?1 AND lease_state!='accepted'",[&r.binding])?;
            tx.execute("UPDATE inbox SET processing_state='retired',payload='{}' WHERE binding=?1 AND kind='probe' AND processing_state!='done'",[&r.binding])?;
            tx.execute("UPDATE outbound_attempts SET state='retired' WHERE binding=?1 AND state IN ('claimed','started','unknown') AND (binding,lane,delivery_id) IN (SELECT binding,lane,id FROM inbox WHERE kind='probe')",[&r.binding])?;
            tx.execute(
                "DELETE FROM outbound_publications WHERE binding=?1",
                [&r.binding],
            )?;
            tx.execute("DELETE FROM outbound_polls WHERE binding=?1", [&r.binding])?;
            tx.execute("UPDATE outbound_transports SET machine_generation=?2,fingerprint=?3,scope=?4,sequence=0,accepted_sequence=0,accepted_digest=NULL WHERE binding=?1",params![r.binding,a.machine_generation,a.credential_fingerprint,key])?;
            (key, consumer)
        }
    } else {
        let fleet_key =
            canonical::digest(&serde_json::json!({"side":r.side_id,"fleet":r.fleet_id}))?;
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM outbound_transports WHERE fleet_key=?1)",
            [&fleet_key],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(Error::Generation);
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM bindings WHERE id=?1)",
            [&r.binding],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(Error::Generation);
        }
        if tx.query_row("SELECT COUNT(*) FROM bindings", [], |row| {
            row.get::<_, i64>(0)
        })? >= max
        {
            return Err(Error::Capacity);
        }
        let key = random_key()?;
        let consumer = consumer()?;
        tx.execute(
            "INSERT INTO bindings(id,generation) VALUES(?1,?2)",
            params![r.binding, r.registration_generation],
        )?;
        tx.execute("INSERT INTO outbound_transports(binding,identity,consumer,machine_generation,fingerprint,scope,fleet_key) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![r.binding,identity,consumer,a.machine_generation,a.credential_fingerprint,key,fleet_key])?;
        (key, consumer)
    };
    Ok(Reply::Scope(TransportScope {
        binding: r.binding.clone(),
        generation: a.machine_generation,
        key,
        consumer,
    }))
}
fn receive(
    tx: &Transaction<'_>,
    poll: PollTicket,
    d: LeasedDelivery,
    now: u64,
    max: i64,
    byte_limit: i64,
) -> Result<Reply, Error> {
    scope(tx, &poll.scope)?;
    if d.machine_generation != poll.scope.generation || d.lane != poll.lane {
        return Err(Error::Generation);
    }
    let prior_poll: Option<Option<String>> = tx
        .query_row(
            "SELECT response_digest FROM outbound_polls WHERE binding=?1 AND lane=?2 AND ticket=?3",
            params![poll.scope.binding, poll.lane.as_str(), poll.key],
            |r| r.get(0),
        )
        .optional()?;
    let Some(prior_poll) = prior_poll else {
        return Err(Error::State);
    };
    let response_digest = canonical::transport_digest(
        &serde_json::json!({"id":d.id,"lane":d.lane,"kind":d.kind,"payload":d.payload,"token":d.token,"expires":d.expires_at_ms,"generation":d.machine_generation}),
    )?;
    if prior_poll.is_some_and(|hash| hash != response_digest) {
        return Err(Error::Conflict);
    }
    let digest = canonical::transport_digest(
        &serde_json::json!({"lane":d.lane,"kind":d.kind,"payload":d.payload}),
    )?;
    let prior:Option<(String,String,Option<u64>,Option<String>)>=tx.query_row("SELECT digest,receipt,lease_generation,lease_token FROM inbox WHERE binding=?1 AND lane=?2 AND id=?3",params![poll.scope.binding,d.lane.as_str(),d.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let receipt = if let Some((saved, receipt, lease_generation, token)) = prior {
        if saved != digest {
            return Err(Error::Conflict);
        }
        if lease_generation != Some(d.machine_generation) || token.as_deref() != Some(&d.token) {
            tx.execute("UPDATE inbox SET lease_generation=?4,lease_token=?5,lease_expires=?6,lease_state='received' WHERE binding=?1 AND lane=?2 AND id=?3",params![poll.scope.binding,d.lane.as_str(),d.id,d.machine_generation,d.token,d.expires_at_ms])?;
        }
        serde_json::from_str(&receipt)?
    } else {
        if tx.query_row("SELECT COUNT(*) FROM inbox", [], |r| r.get::<_, i64>(0))? >= max {
            return Err(Error::Capacity);
        }
        let generation: u64 = tx.query_row(
            "SELECT generation FROM bindings WHERE id=?1",
            [&poll.scope.binding],
            |r| r.get(0),
        )?;
        let receipt = Receipt {
            id: d.id.clone(),
            lane: d.lane,
            generation,
            digest,
            received_at_ms: now,
            state: CustodyState::Received,
        };
        tx.execute("INSERT INTO inbox(binding,lane,id,generation,digest,payload,receipt,kind,origin_machine_generation,lease_generation,lease_token,lease_expires,lease_state,processing_state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,?10,?11,'received','pending')",params![poll.scope.binding,d.lane.as_str(),d.id,generation,receipt.digest,serde_json::to_string(&d.payload)?,serde_json::to_string(&receipt)?,kind_text(d.kind),d.machine_generation,d.token,d.expires_at_ms])?;
        capacity(tx, byte_limit)?;
        receipt
    };
    tx.execute(
        "UPDATE outbound_polls SET response_digest=?3 WHERE binding=?1 AND lane=?2",
        params![poll.scope.binding, poll.lane.as_str(), response_digest],
    )?;
    Ok(Reply::Received(receipt))
}
fn ack(tx: &Transaction<'_>, ticket: AckTicket, response: AckResponse) -> Result<Reply, Error> {
    scope(tx, &ticket.scope)?;
    let row:Option<(u64,String,String)>=tx.query_row("SELECT lease_generation,lease_token,lease_state FROM inbox WHERE binding=?1 AND lane=?2 AND id=?3",params![ticket.scope.binding,ticket.lane.as_str(),ticket.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((generation, token, state)) = row else {
        return Err(Error::NotFound);
    };
    if generation != ticket.scope.generation || token != ticket.token {
        return Ok(Reply::Ack(AckResolution::Replaced));
    }
    if state == "accepted" {
        return Ok(Reply::Ack(AckResolution::Accepted));
    }
    let (state, resolution) = match response {
        AckResponse::Accepted => ("accepted", AckResolution::Accepted),
        AckResponse::Unknown => ("unknown", AckResolution::Unknown),
        AckResponse::StaleLease => ("stale", AckResolution::Reclaim),
    };
    tx.execute(
        "UPDATE inbox SET lease_state=?4 WHERE binding=?1 AND lane=?2 AND id=?3",
        params![ticket.scope.binding, ticket.lane.as_str(), ticket.id, state],
    )?;
    Ok(Reply::Ack(resolution))
}
fn claim(
    tx: &Transaction<'_>,
    s: TransportScope,
    lane: Lane,
    id: String,
    lease_ms: u64,
    now: u64,
    max: i64,
) -> Result<Reply, Error> {
    scope(tx, &s)?;
    let digest = canonical::digest(&serde_json::json!({"lane":lane,"lease_ms":lease_ms}))?;
    let previous:Option<(String,String,String)>=tx.query_row("SELECT command_digest,capability,state FROM outbound_attempts WHERE binding=?1 AND id=?2",params![s.binding,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((saved, key, state)) = previous {
        if saved != digest {
            return Err(Error::Conflict);
        }
        if !["claimed", "started", "unknown", "completed"].contains(&state.as_str()) {
            return Err(Error::State);
        }
        return Ok(Reply::Claim(Some(ClaimTicket {
            binding: s.binding,
            id,
            key,
        })));
    }
    let row:Option<(String,String,String,u64)>=tx.query_row("SELECT id,processing_state,lease_state,retry_at FROM inbox WHERE binding=?1 AND lane=?2 AND processing_state NOT IN ('done','retired') ORDER BY rowid LIMIT 1",params![s.binding,lane.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((delivery, state, lease, retry)) = row else {
        return Ok(Reply::Claim(None));
    };
    if state != "pending" || !["accepted", "retired"].contains(&lease.as_str()) || retry > now {
        return Ok(Reply::Claim(None));
    }
    if tx.query_row("SELECT COUNT(*) FROM outbound_attempts", [], |r| {
        r.get::<_, i64>(0)
    })? >= max
    {
        return Err(Error::Capacity);
    }
    let deadline = now
        .checked_add(lease_ms)
        .filter(|value| *value <= JSON_SAFE_MAX)
        .ok_or(InvalidInput("custody deadline exceeds safe range"))?;
    let key = random_key()?;
    tx.execute("INSERT INTO outbound_attempts(binding,id,lane,delivery_id,command_digest,capability,state,deadline) VALUES(?1,?2,?3,?4,?5,?6,'claimed',?7)",params![s.binding,id,lane.as_str(),delivery,digest,key,deadline])?;
    tx.execute(
        "UPDATE inbox SET processing_state='claimed' WHERE binding=?1 AND lane=?2 AND id=?3",
        params![s.binding, lane.as_str(), delivery],
    )?;
    Ok(Reply::Claim(Some(ClaimTicket {
        binding: s.binding,
        id,
        key,
    })))
}
fn start(tx: &Transaction<'_>, ticket: ClaimTicket, now: u64) -> Result<Reply, Error> {
    let row:Option<(String,u64,String,String,u64,String)>=tx.query_row("SELECT a.state,a.deadline,i.receipt,i.kind,i.origin_machine_generation,i.payload FROM outbound_attempts a JOIN inbox i ON i.binding=a.binding AND i.lane=a.lane AND i.id=a.delivery_id WHERE a.binding=?1 AND a.id=?2 AND a.capability=?3",params![ticket.binding,ticket.id,ticket.key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((state, deadline, receipt, kind_name, origin, payload)) = row else {
        return Err(Error::State);
    };
    // Start returns work exactly once; a lost result is inspected, never executed twice.
    if state != "claimed" || deadline <= now {
        return Err(Error::State);
    }
    tx.execute(
        "UPDATE outbound_attempts SET state='started' WHERE binding=?1 AND id=?2",
        params![ticket.binding, ticket.id],
    )?;
    tx.execute("UPDATE inbox SET processing_state='started' WHERE (binding,lane,id) IN (SELECT binding,lane,delivery_id FROM outbound_attempts WHERE binding=?1 AND id=?2)",params![ticket.binding,ticket.id])?;
    Ok(Reply::Started(StartedWork {
        ticket,
        delivery: serde_json::from_str(&receipt)?,
        kind: kind(&kind_name)?,
        origin_machine_generation: origin,
        payload: serde_json::from_str(&payload)?,
    }))
}
fn finish(
    tx: &Transaction<'_>,
    ticket: &ClaimTicket,
    result: &Value,
    inspection: bool,
) -> Result<String, Error> {
    let row:Option<(String,Option<String>)>=tx.query_row("SELECT state,result_digest FROM outbound_attempts WHERE binding=?1 AND id=?2 AND capability=?3",params![ticket.binding,ticket.id,ticket.key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((state, previous)) = row else {
        return Err(Error::State);
    };
    let digest = canonical::transport_digest(result)?;
    if state == "completed" {
        return if previous.as_deref() == Some(&digest) {
            Ok(digest)
        } else {
            Err(Error::Conflict)
        };
    }
    if state != if inspection { "unknown" } else { "started" } {
        return Err(Error::State);
    }
    tx.execute("UPDATE outbound_attempts SET state='completed',result=?3,result_digest=?4 WHERE binding=?1 AND id=?2",params![ticket.binding,ticket.id,serde_json::to_string(result)?,digest])?;
    // Compact only completed payload. Arbitrary-ID dedup tombstones remain and
    // count toward the finite record cap; no pending row is dropped for capacity.
    tx.execute("UPDATE inbox SET processing_state='done',payload='{}' WHERE (binding,lane,id) IN (SELECT binding,lane,delivery_id FROM outbound_attempts WHERE binding=?1 AND id=?2)",params![ticket.binding,ticket.id])?;
    Ok(digest)
}
fn publication(
    tx: &Transaction<'_>,
    s: &TransportScope,
) -> Result<Option<PublicationTicket>, Error> {
    let row: Option<(u64, String, String)> = tx
        .query_row(
            "SELECT sequence,digest,body FROM outbound_publications WHERE binding=?1",
            [&s.binding],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.map(|(sequence, digest, body)| PublicationTicket {
        scope: s.clone(),
        sequence,
        digest,
        body,
    }))
}
fn publication_matches(tx: &Transaction<'_>, ticket: &PublicationTicket) -> Result<(), Error> {
    let pending = publication(tx, &ticket.scope)?.ok_or(Error::State)?;
    if pending.sequence != ticket.sequence
        || pending.digest != ticket.digest
        || pending.body != ticket.body
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
fn freeze(
    tx: &Transaction<'_>,
    s: TransportScope,
    mut body: Value,
    limit: i64,
) -> Result<Reply, Error> {
    scope(tx, &s)?;
    // Only a completed probe owned by this exact machine incarnation may enter
    // its publication. Matrix source/member proof remains a host-adapter gate.
    for receipt in body
        .get("probeReceipts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let digest = canonical::transport_digest(receipt)?;
        let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outbound_attempts a JOIN inbox i ON i.binding=a.binding AND i.lane=a.lane AND i.id=a.delivery_id WHERE a.binding=?1 AND a.state='completed' AND a.result_digest=?2 AND i.kind='probe' AND i.origin_machine_generation=?3)",params![s.binding,digest,s.generation],|r|r.get(0))?;
        if !current {
            return Err(Error::Generation);
        }
    }
    let pending = publication(tx, &s)?;
    let sequence = if let Some(ticket) = &pending {
        ticket.sequence
    } else {
        let last: u64 = tx.query_row(
            "SELECT sequence FROM outbound_transports WHERE binding=?1",
            [&s.binding],
            |r| r.get(0),
        )?;
        last.checked_add(1)
            .filter(|value| *value <= JSON_SAFE_MAX)
            .ok_or(Error::Capacity)?
    };
    let map = body
        .as_object_mut()
        .ok_or(InvalidInput("publication must be object"))?;
    map.insert("v".into(), Value::from(2));
    map.insert("sequence".into(), Value::from(sequence));
    map.insert("generation".into(), Value::from(s.generation));
    let encoded = canonical::encode_transport(&body)?;
    if encoded.len() > 1024 * 1024 {
        return Err(Error::Capacity);
    }
    let digest = canonical::transport_digest(&body)?;
    if let Some(ticket) = pending {
        if ticket.digest != digest || ticket.body != encoded {
            return Err(Error::Conflict);
        }
        return Ok(Reply::Publication(Some(ticket)));
    }
    tx.execute(
        "UPDATE outbound_transports SET sequence=?2 WHERE binding=?1",
        params![s.binding, sequence],
    )?;
    tx.execute("INSERT INTO outbound_publications(binding,sequence,digest,body,state) VALUES(?1,?2,?3,?4,'ready')",params![s.binding,sequence,digest,encoded])?;
    capacity(tx, limit)?;
    Ok(Reply::Publication(Some(PublicationTicket {
        scope: s,
        sequence,
        digest,
        body: encoded,
    })))
}
fn publication_result(
    tx: &Transaction<'_>,
    ticket: PublicationTicket,
    response: PublicationResponse,
) -> Result<Reply, Error> {
    scope(tx, &ticket.scope)?;
    let accepted: (u64, Option<String>) = tx.query_row(
        "SELECT accepted_sequence,accepted_digest FROM outbound_transports WHERE binding=?1",
        [&ticket.scope.binding],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if accepted.0 == ticket.sequence {
        if accepted.1.as_deref() != Some(&ticket.digest) {
            return Err(Error::Conflict);
        }
        return Ok(Reply::PublicationRecorded);
    }
    publication_matches(tx, &ticket)?;
    match response {
        PublicationResponse::Accepted => {
            tx.execute("UPDATE outbound_transports SET accepted_sequence=?2,accepted_digest=?3 WHERE binding=?1",params![ticket.scope.binding,ticket.sequence,ticket.digest])?;
            tx.execute(
                "DELETE FROM outbound_publications WHERE binding=?1",
                [&ticket.scope.binding],
            )?;
        }
        PublicationResponse::Unknown | PublicationResponse::Rejected => {
            let state = if matches!(response, PublicationResponse::Unknown) {
                "unknown"
            } else {
                "rejected"
            };
            tx.execute(
                "UPDATE outbound_publications SET state=?2 WHERE binding=?1",
                params![ticket.scope.binding, state],
            )?;
        }
    }
    Ok(Reply::PublicationRecorded)
}
