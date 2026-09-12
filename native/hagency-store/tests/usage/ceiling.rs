use super::*;

/// Replay the retained-JavaScript ceiling oracle: every vector names its
/// sources' observations (absolute ms clocks) and the figures the JS ledger
/// plus the `backend-v2.js` drawn rule produce for the current period.
fn vectors() -> serde_json::Value {
    serde_json::from_str(include_str!("../fixtures/ceiling-vectors.json")).unwrap()
}

fn named<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    value["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap()
}

fn totals(value: &serde_json::Value) -> UsageObservation {
    claude(
        value["input"].as_u64().unwrap(),
        value["output"].as_u64().unwrap(),
        value["cacheWrite"].as_u64().unwrap(),
        value["cacheRead"].as_u64().unwrap(),
    )
}

/// Record one vector's observations exactly as the writer would: every source
/// is bound while its Started scope is fresh, then observations land on a
/// single merged chronological timeline so the store clock never moves back.
fn observe(f: &mut Fixture, vector: &serde_json::Value) -> u64 {
    let sources: Vec<_> = (0..vector["sources"].as_array().unwrap().len())
        .map(|_| f.start().2)
        .collect();
    let mut events: Vec<(u64, usize, &serde_json::Value)> = Vec::new();
    for (index, source) in vector["sources"].as_array().unwrap().iter().enumerate() {
        for observation in source["observations"].as_array().unwrap() {
            events.push((observation["at"].as_u64().unwrap(), index, observation));
        }
    }
    events.sort_by_key(|(at, _, _)| *at);
    for (step, (at, index, observation)) in events.iter().enumerate() {
        f.db.record_usage_observation(
            &sources[*index],
            &format!("call_{step}"),
            &totals(&observation["totals"]),
            *at,
        )
        .unwrap();
    }
    events.last().map(|(at, _, _)| *at).unwrap_or_default()
}

fn report(f: &Fixture, at: u64) -> CeilingReport {
    let pool = resource("usage_pool", "usage_seat", 1000);
    f.db.resource_ceiling(&pool.id(), at).unwrap()
}

fn assert_report_matches(f: &Fixture, vector: &serde_json::Value, at: u64) {
    let r = report(f, at);
    let expected = &vector["expected"];
    assert_eq!(r.reserved, expected["reserved"].as_u64().unwrap());
    assert_eq!(r.spent, expected["spent"].as_u64());
    assert_eq!(r.consumed, expected["consumed"].as_u64());
    assert_eq!(r.drawn, expected["drawn"].as_u64().unwrap());
    assert_eq!(
        r.spend_period_key.as_deref(),
        expected["spendPeriodKey"].as_str()
    );
}

#[test]
fn native_ceiling_draws_fresh_tokens_only() {
    let value = vectors();
    // The lockout that motivated the ruling: 13.6M consumed against a 10M
    // ceiling of which only 681k is fresh work. The draw must be the fresh
    // figure, never the parity total.
    let mut f = Fixture::new(Framework::Claude);
    let at = observe(&mut f, named(&value, "lockout-cache-swamp"));
    let r = report(&f, at);
    assert_eq!(r.spent, Some(681_089));
    assert_eq!(r.consumed, Some(13_609_601));
    assert_eq!(r.drawn, 681_089);
    assert_eq!(r.period, UsagePeriodKind::Monthly);
    assert_eq!(r.ceiling_tokens, Some(1000));
    assert_eq!(r.preset_name, "usage_pool");
    assert_eq!(r.spend_period_key.as_deref(), Some("2026-08"));
    // Cache-read growth moves consumption and never the draw.
    let mut f = Fixture::new(Framework::Claude);
    let at = observe(&mut f, named(&value, "cache-read-growth-never-draws"));
    let r = report(&f, at);
    assert_eq!(r.spent, Some(150));
    assert_eq!(r.consumed, Some(5150));
    assert_eq!(r.drawn, 150);
}

#[test]
fn native_ceiling_vectors_match_javascript() {
    let value = vectors();
    for vector in value["vectors"].as_array().unwrap() {
        let mut f = Fixture::new(Framework::Claude);
        let last = observe(&mut f, vector);
        let at = vector["queryAt"].as_u64().unwrap_or(last);
        assert_report_matches(&f, vector, at);
    }
}

#[test]
fn native_ceiling_unknown_spend_falls_back_to_commitments() {
    // No bucket for this period means nobody measured it, not that nothing
    // was consumed: spend stays unknown and the commitment stands alone.
    let value = vectors();
    let mut f = Fixture::new(Framework::Claude);
    let vector = named(&value, "month-roll-unknown");
    let last = observe(&mut f, vector);
    // The vector queries after the period rolled over, where no bucket exists;
    // at the observation instant itself the bucket is present and measured.
    let at = vector["queryAt"].as_u64().unwrap_or(last);
    let r = report(&f, at);
    assert_eq!(r.spent, None);
    assert_eq!(r.consumed, None);
    assert_eq!(r.spend_period_key, None);
    assert_eq!(r.reserved, 100);
    assert_eq!(r.drawn, 100);
    // The same holds before anything is measured at all.
    let f = Fixture::new(Framework::Claude);
    let r = report(&f, at);
    assert_eq!(r.spent, None);
    assert_eq!(r.drawn, r.reserved);
}

#[test]
fn native_ceiling_draw_takes_max_of_committed_and_measured() {
    let value = vectors();
    // Commitments above measurement: committed allocations are the binding draw.
    let mut f = Fixture::new(Framework::Claude);
    let at = observe(&mut f, named(&value, "committed-binding"));
    let r = report(&f, at);
    assert_eq!(r.spent, Some(40));
    assert_eq!(r.drawn, 100);
    // Measurement above commitments: measured spend is the binding draw.
    let mut f = Fixture::new(Framework::Claude);
    let at = observe(&mut f, named(&value, "fresh-exhaustion"));
    let r = report(&f, at);
    assert_eq!(r.spent, Some(10_000_000));
    assert_eq!(r.drawn, 10_000_000);
}
