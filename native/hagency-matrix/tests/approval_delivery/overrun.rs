use super::*;

/// ADR-149 fail-first: a delivery that overruns its budget reports
/// `Error::Timeout`, never a manufactured `Cancelled`. The SDK owner is held
/// (`Command::Hold`, phase 3 — before any wire write) so the delivery parks;
/// tokio time advances past the 45 s delivery ceiling; the hold releases; the
/// send must resolve `Err(Error::Timeout)` with the denial row carrying the
/// "timed out" reason. On the pre-fix product this is red: the old deadline
/// arm self-cancelled the child token and awaited the work, so the parked
/// delivery resolved `Cancelled`, never `Timeout`.
#[tokio::test]
async fn native_private_approval_delivery_overrun_is_a_timeout() {
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let card = f.card(1, true).await;
    let request_id = card.target().request_id.clone();
    // Park the delivery before any wire write, exactly as privacy.rs does.
    let (reached, wait) = tokio::sync::oneshot::channel();
    let (release, resume) = tokio::sync::oneshot::channel();
    let handle = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .approval_delivery_handle();
    handle
        .command(Command::Hold(crate::sdk::approval_delivery::ReplyHold {
            phase: 3,
            reached,
            release: resume,
            lose: false,
        }))
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let mut operation = Box::pin(
        f.collector
            .send_private_approval_card(card.clone(), &cancel),
    );
    tokio::pin!(wait);
    loop {
        tokio::select! {r=&mut operation=>panic!("delivery escaped the hold: {r:?}"),r=&mut wait=>{r.unwrap();break;},request=f.fake.next()=>respond(request,&mut f.peer).await}
    }
    // The delivery is parked mid-flight. Freeze time, run the 45 s delivery
    // clock out, and thaw: the deadline arm must fire — with a Timeout.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(46)).await;
    tokio::time::resume();
    release.send(()).unwrap();
    let result = drive(operation, &mut f.fake, &mut f.peer).await;
    assert!(
        matches!(result, Err(Error::Timeout)),
        "an overrun is a timeout, got {result:?}"
    );
    assert_eq!(f.peer.shares, 0);
    assert!(f.peer.events.is_empty());
    // Fail-closed (ADR-137): the denial row names the timeout class.
    let summary = f
        .base
        .store
        .approval_summary(request_id.clone())
        .await
        .unwrap();
    assert_eq!(summary.state, "decided");
    assert_eq!(summary.choice, Some(ApprovalChoice::Deny));
    let conn = rusqlite::Connection::open(f.base.root.path().join("domain").join("domain.sqlite3"))
        .unwrap();
    let reason: String = conn
        .query_row(
            "SELECT denial_reason FROM approval_verdict_receipts WHERE request_id=?1",
            [&request_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        reason.ends_with("Matrix operation timed out"),
        "the denial names the timeout class: {reason}"
    );
    f.fake.no_request().await;
    // The known close-time coupling (ADR-149 briefing): a delivery still in
    // flight at close makes close report unknown; here the send has settled,
    // so close is honest either way and the domain shuts down cleanly.
    let close = f.collector.close().await;
    assert!(close.is_ok(), "actual close: {close:?}");
    common::shutdown_domain(&f.base.store, "delivery-overrun").await;
    f.fake.close().await;
}
