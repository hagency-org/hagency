use super::*;
use hagency_core::project::EngagementState;

/// THE LOCKOUT, ported: a ceiling swamped by cache reads still approves work.
/// 13.6M consumed against a 10M ceiling of which only 681k is fresh. Under a
/// consumed-total rule this agent had zero headroom and every allocation
/// threw; under the drawn rule the figure is what fresh tokens actually drew.
#[test]
fn native_ceiling_admission_uses_drawn_not_consumed() {
    let mut f = Fixture::with_ceiling(Framework::Claude, 10_000_000);
    let pool = resource("usage_pool", "usage_seat", 10_000_000);
    let (_cap, _started, source) = f.start();
    let at = 2000u64;
    f.db.record_usage_observation(
        &source,
        "lockout_observation",
        &claude(604_823, 76_266, 0, 12_928_512),
        at,
    )
    .unwrap();
    let report = f.db.resource_ceiling(&pool.id(), at).unwrap();
    assert_eq!(report.spent, Some(681_089));
    assert_eq!(report.consumed, Some(13_609_601));
    assert!(report.consumed.unwrap() > report.ceiling_tokens.unwrap());
    // The approval SUCCEEDS: only fresh tokens drew the ceiling down.
    let ask = request("lockout_request", "Lockout", &pool, 1_000_000);
    f.db.admit(&proof(&ask), 3000).unwrap();
    let approved = f.db.approve("approve_lockout", &proof(&ask), 3000).unwrap();
    assert_eq!(approved.state, EngagementState::Reserved);
}

/// The counter-case that makes the rule enforcement, not removal: the same
/// total in FRESH kinds genuinely exhausts the ceiling and the allocation is
/// refused, naming measured spend as the binding draw.
#[test]
fn native_ceiling_admission_refuses_fresh_exhaustion() {
    let mut f = Fixture::with_ceiling(Framework::Claude, 10_000_000);
    let pool = resource("usage_pool", "usage_seat", 10_000_000);
    let (_cap, _started, source) = f.start();
    f.db.record_usage_observation(
        &source,
        "exhaustion_observation",
        &claude(9_500_000, 500_000, 0, 0),
        2000,
    )
    .unwrap();
    let ask = request("exhaustion_request", "Exhaustion", &pool, 1_000_000);
    f.db.admit(&proof(&ask), 3000).unwrap();
    match f.db.approve("approve_exhaustion", &proof(&ask), 3000) {
        Err(Error::OverCommit { message }) => {
            assert!(
                message.contains("measured spend is what is binding"),
                "{message}"
            );
            assert!(message.contains("10.0M"), "{message}");
            assert!(message.contains("usage_pool"), "{message}");
        }
        Err(Error::NoCeiling) => panic!("a declared ceiling must not read as unknown"),
        Err(Error::InsufficientCapacity) => {
            panic!("the ceiling side is binding here, not the shared seat")
        }
        other => panic!("expected the fresh-exhaustion refusal, got {other:?}"),
    }
}

/// AND THE HEADROOM FIGURE IS PUBLISHED, so no client re-derives
/// `ceiling - committed`: `remaining = ceiling - max(reserved, spent)` with
/// the display total kept visible beside it, unconsumed by enforcement.
#[test]
fn native_ceiling_publishes_headroom_after_approval() {
    let mut f = Fixture::with_ceiling(Framework::Claude, 10_000_000);
    let pool = resource("usage_pool", "usage_seat", 10_000_000);
    let (_cap, _started, source) = f.start();
    f.db.record_usage_observation(
        &source,
        "headroom_observation",
        &claude(604_823, 76_266, 0, 12_928_512),
        2000,
    )
    .unwrap();
    let ask = request("headroom_request", "Headroom", &pool, 1_000_000);
    f.db.admit(&proof(&ask), 3000).unwrap();
    f.db.approve("approve_headroom", &proof(&ask), 3000)
        .unwrap();
    let published = f.db.usage_report(&f.engagement, 3000).unwrap().ceiling;
    // reserved = 100 (fixture base) + 1_000_000 (just approved);
    // spent = 681_089 fresh; drawn = max of the two.
    assert_eq!(published.tokens_drawn, 1_000_100);
    assert_eq!(published.tokens_used, Some(13_609_601));
    // Not the parity total: consumption exceeds the ceiling while headroom
    // stays positive, because cache reads never draw.
    assert_eq!(published.remaining_tokens, Some(8_999_900));
}
