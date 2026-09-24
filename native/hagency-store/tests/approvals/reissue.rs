use super::*;

/// PC-C5 (ADR-110 amendment): an undeliverable private card's fate is
/// permanent-uncertain. The stored row stays terminal at PC-C1's word —
/// state='decided', choice='deny' — with the kind-deny receipt naming the
/// failed send, and the fate is read from the store side alone: the summary
/// plus `delivery_denial_reason` are the one fact every surface renders.
/// "Permanent-uncertain" is what the operator is told, never a stored state
/// word: the stored 'uncertain' is the applying→uncertain recovery sweep's,
/// and 'invalidated' stays an unused CHECK word until a visible re-issue
/// follow-on lands. No store entry point mints a fresh card, re-issues the
/// same request id, re-queues the send or re-sends the packet; a future
/// re-issue is a NEW request id linking the old one (ADR text only).
#[test]
fn native_private_approval_lost_send_is_permanently_visible_as_uncertain() {
    let mut f = Fixture::new(true);
    let pending = f.admit(0, 1);
    assert_eq!(pending.state, "pending");
    // The card exists while the request is pending — PC-C1's send leg held
    // exactly this one; a lost send is a send of this card, never a reason
    // to rebuild it (ADR-110's no-reconstruction rule).
    let card =
        f.db.private_approval_card(&pending.id, 11000, 1011)
            .unwrap();
    // PC-C1's fail-closed leg: the send did not reach Accepted, so the
    // denial receipt is minted with the reason naming the failed send.
    let reason = "private approval card send failed: Matrix operation timed out";
    let denied =
        f.db.deny_for_failed_delivery(&pending.id, reason, 1012)
            .unwrap();
    // The fate, read from the store side alone: decided/deny plus the
    // kind-deny receipt row. This pair is the permanent-uncertain fact.
    assert_eq!(denied.state, "decided");
    assert_eq!(denied.choice, Some(ApprovalChoice::Deny));
    assert_eq!(
        f.db.delivery_denial_reason(&pending.id).unwrap().as_deref(),
        Some(reason)
    );
    let summary = f.db.approval_summary(&pending.id).unwrap();
    assert_eq!(summary.state, "decided");
    assert_eq!(summary.choice, Some(ApprovalChoice::Deny));
    // The stored word is PC-C1's deny word — never 'uncertain' and never
    // 'invalidated' (both remain what migration 013 defined them for).
    assert_eq!(
        f.sql()
            .query_row(
                "SELECT state FROM owner_approvals WHERE id=?1",
                [&pending.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "decided"
    );
    // No fresh card: the mint refuses a row that is no longer pending, so
    // the lost card cannot be replaced under the same request id.
    assert!(matches!(
        f.db.private_approval_card(&pending.id, 11000, 1013),
        Err(Error::RunnerAuthority)
    ));
    // The old card cannot be re-checked into authority either: the same
    // refusals stand unchanged (intake_target's pending gate), and a packet
    // read never confers send authority.
    assert!(matches!(
        f.db.check_private_approval_card(&card, 1013),
        Err(Error::RunnerAuthority)
    ));
    // The same request id cannot be re-issued: the identical request is
    // idempotent on its source and returns the SAME terminal row — never a
    // fresh pending card, never a re-queued send for this id.
    let rows_before = count(&f.sql(), "owner_approvals");
    let reissued =
        f.db.request_owner_approval(&f.caps[0], &f.input(0, 1), 1014)
            .unwrap();
    assert_eq!(reissued.id, pending.id);
    assert_eq!(reissued.state, "decided");
    assert_eq!(reissued.choice, Some(ApprovalChoice::Deny));
    assert_eq!(count(&f.sql(), "owner_approvals"), rows_before);
    // No second decision overwrites the recorded one — not even the owner's
    // own verdict on the card that never arrived.
    let stale = f.verdict(&pending.id, ApprovalChoice::Once, "late");
    assert!(matches!(
        f.db.observe_owner_verdict(&stale, 1015),
        Err(Error::RunnerAuthority)
    ));
    assert_eq!(
        f.db.approval_summary(&pending.id).unwrap().choice,
        Some(ApprovalChoice::Deny)
    );
    // The kind-deny receipt is PC-C1's alone: an owner-verdict receipt
    // (NULL denial_reason) never reads as a delivery denial, so the fate is
    // never invented for a request whose send did not fail.
    let answered = f.admit(0, 2);
    f.choose(&answered.id, ApprovalChoice::Once);
    assert_eq!(
        f.db.approval_summary(&answered.id).unwrap().state,
        "decided"
    );
    assert_eq!(
        f.db.delivery_denial_reason(&answered.id).unwrap(),
        None,
        "an owner verdict is not a delivery failure"
    );
}
