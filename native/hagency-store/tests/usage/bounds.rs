use super::*;

// Populate finite historical table shapes in a single private fixture transaction.
// These artificial rows exercise capacity only; they are not attribution evidence.
fn historical_engagement(f: &mut Fixture) -> String {
    let p = proof(&request(
        "historical",
        "Historical",
        &resource("usage_pool", "usage_seat", 1000),
        1,
    ));
    f.db.admit(&p, 1006).unwrap().id
}
fn seed_sources(f: &Fixture, source: &UsageSource, engagement: &str, total: u64) {
    let mut sql = f.sql();
    let tx = sql.transaction().unwrap();
    for i in 1..total {
        tx.execute("INSERT INTO usage_sources(id,dispatch_id,fence,engagement_id,identity_digest,framework,attribution,high_water,latest_counts) SELECT ?1,dispatch_id,?2,?3,identity_digest,framework,attribution,'{\"input\":null,\"output\":null,\"cacheWrite\":null,\"cacheRead\":null}','null' FROM usage_sources WHERE id=?4",rusqlite::params![format!("historical_source_{i}"),i+1000,engagement,source.id()]).unwrap();
    }
    tx.commit().unwrap();
}
fn seed_receipts(f: &Fixture, source: &UsageSource, target: &str, total: u64) {
    let mut sql = f.sql();
    let tx = sql.transaction().unwrap();
    for i in 1..total {
        tx.execute("INSERT INTO usage_receipts(source_id,call_id,digest,observation,response) SELECT ?1,?2,digest,observation,response FROM usage_receipts WHERE source_id=?3 AND call_id='original'",rusqlite::params![target,format!("historical_receipt_{i}"),source.id()]).unwrap();
    }
    tx.commit().unwrap();
}
fn seed_periods(f: &Fixture, engagement: &str, total: u64) {
    let mut sql = f.sql();
    let tx = sql.transaction().unwrap();
    for i in 2..total {
        tx.execute("INSERT INTO usage_periods(engagement_id,granularity,period_key,observed_growth,known_growth,incomplete,observations) VALUES(?1,'daily',?2,'{\"input\":0,\"output\":0,\"cacheWrite\":0,\"cacheRead\":0}','{\"input\":0,\"output\":0,\"cacheWrite\":0,\"cacheRead\":0}',0,1)",rusqlite::params![engagement,format!("historical_day_{i}")]).unwrap();
    }
    tx.commit().unwrap();
}

#[test]
fn native_usage_capacity_and_rollback() {
    for global in [false, true] {
        let mut f = Fixture::new(Framework::Codex);
        let (_, _, source) = f.start();
        f.db.record_usage_observation(&source, "original", &codex(1, 0, 0), 2000)
            .unwrap();
        let (cap, admission) = f.prepare();
        let started =
            f.db.start_owned_dispatch(&cap, admission.fingerprint(), 1004)
                .unwrap();
        let engagement = if global {
            historical_engagement(&mut f)
        } else {
            f.engagement.clone()
        };
        seed_sources(
            &f,
            &source,
            &engagement,
            if global {
                MAX_USAGE_SOURCES
            } else {
                MAX_ENGAGEMENT_USAGE_SOURCES
            },
        );
        assert!(matches!(
            f.db.bind_usage_source(&cap, &started, 1005),
            Err(Error::Capacity)
        ));
        // Source-table exhaustion cannot discard admitted identity or prohibit
        // its otherwise in-bounds growth; no replacement source is minted.
        f.db.record_usage_observation(&source, "growth", &codex(5, 0, 0), 2001)
            .unwrap();
        assert_eq!(
            f.db.usage_source(&source).unwrap().high_water.input,
            Some(5)
        );
        assert!(
            f.db.record_usage_observation(&source, "original", &codex(1, 0, 0), 0)
                .unwrap()
                .replayed
        );
    }
    for global in [false, true] {
        let mut f = Fixture::new(Framework::Codex);
        let (_, _, source) = f.start();
        f.db.record_usage_observation(&source, "original", &codex(1, 0, 0), 2000)
            .unwrap();
        let target = if global { f.start().2 } else { source.clone() };
        seed_receipts(
            &f,
            &source,
            target.id(),
            if global {
                MAX_USAGE_RECEIPTS
            } else {
                MAX_SOURCE_USAGE_RECEIPTS
            },
        );
        assert!(matches!(
            f.db.record_usage_observation(&source, "overflow", &codex(5, 0, 0), 2001),
            Err(Error::Capacity)
        ));
        assert_eq!(
            f.db.usage_source(&source).unwrap().high_water.input,
            Some(1)
        );
        assert!(
            f.db.record_usage_observation(&source, "original", &codex(1, 0, 0), 0)
                .unwrap()
                .replayed
        );
    }
    for global in [false, true] {
        let mut f = Fixture::new(Framework::Codex);
        let (_, _, source) = f.start();
        f.db.record_usage_observation(&source, "original", &codex(1, 0, 0), 2000)
            .unwrap();
        let engagement = if global {
            historical_engagement(&mut f)
        } else {
            f.engagement.clone()
        };
        seed_periods(
            &f,
            &engagement,
            if global {
                MAX_USAGE_PERIODS
            } else {
                MAX_ENGAGEMENT_USAGE_PERIODS
            },
        );
        assert!(matches!(
            f.db.record_usage_observation(&source, "new_day", &codex(5, 0, 0), 86400000),
            Err(Error::Capacity)
        ));
        assert_eq!(
            f.db.usage_source(&source).unwrap().high_water.input,
            Some(1)
        );
        assert_eq!(f.count("usage_receipts"), 1);
        // Exactly one free row cannot admit a new UTC day AND month. Daily
        // insertion happens first, but monthly refusal rolls it back as well.
        f.sql()
            .execute(
                "DELETE FROM usage_periods WHERE period_key='historical_day_2'",
                [],
            )
            .unwrap();
        let rows = f.count("usage_periods");
        assert!(matches!(
            f.db.record_usage_observation(&source, "new_month", &codex(5, 0, 0), 2678400000),
            Err(Error::Capacity)
        ));
        assert_eq!(f.count("usage_periods"), rows);
        assert_eq!(f.count("usage_receipts"), 1);
        assert!(
            f.db.usage_period(&f.engagement, UsagePeriodKind::Daily, 2678400000)
                .unwrap()
                .is_none()
        );
        // Updating existing buckets does not consume a new period row.
        f.db.record_usage_observation(&source, "existing_day", &codex(3, 0, 0), 2001)
            .unwrap();
        assert_eq!(
            f.db.usage_source(&source).unwrap().high_water.input,
            Some(3)
        );
    }
    let mut f = Fixture::new(Framework::Codex);
    let (_, _, source) = f.start();
    let (_, _, other) = f.start();
    f.db.record_usage_observation(&source, "maximum", &codex(JSON_SAFE_MAX, 0, 0), 2000)
        .unwrap();
    let partial = UsageObservation::parse(
        Framework::Codex,
        r#"{"payload":{"info":{"total_token_usage":{"output_tokens":1,"total_tokens":1}}}}"#,
    )
    .unwrap();
    assert_eq!(partial.counts().unwrap().input, None);
    assert!(matches!(
        f.db.record_usage_observation(&other, "unknown_cannot_hide_overflow", &partial, 2001),
        Err(Error::Capacity)
    ));
    let summary = f.totals();
    assert_eq!(summary.latest_counts.unwrap().input, None);
    assert_eq!(
        summary.known_high_water_lower_bound.unwrap().input,
        JSON_SAFE_MAX
    );
    assert_eq!(f.count("usage_receipts"), 1);
    assert_eq!(f.db.usage_source(&other).unwrap().observations, 0);
}
