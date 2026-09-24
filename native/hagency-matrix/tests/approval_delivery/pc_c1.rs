use super::*;
use crate::approval_delivery::public;

/// PC-C1 selector 1: the redacted public status notice is content-free and
/// non-actionable (ADR-137). The exact status packet posts once to the
/// project room through `PublicFrozen`, never to the private room, and a
/// notice addressed from stale or caller-influenced state is refused by the
/// destination re-derivation.
#[tokio::test]
async fn native_private_approval_public_status_notice() {
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let card = f.card(1, true).await;
    let request_id = card.target().request_id.clone();

    // The packet is the exact status shape: three top-level keys, a five-key
    // status word, and no request material in any byte.
    let notice = public::PublicFrozen::new(&card).unwrap();
    let content = notice.content().unwrap();
    let value: Value = serde_json::from_str(&content).unwrap();
    let obj = value.as_object().unwrap();
    let status = value[public::STATUS_KEY].as_object().unwrap();
    assert_eq!(obj.len(), 3, "exactly three top-level keys: {content}");
    assert_eq!(status.len(), 5, "exactly five status keys: {content}");
    assert_eq!(value["msgtype"], public::NOTICE_MSGTYPE);
    assert_eq!(value[public::STATUS_KEY]["state"], "waiting_for_owner");
    for forbidden in [
        "request_id",
        "requestId",
        "digest",
        "tool",
        "tool_name",
        "preview",
        "scope",
        "scope_key",
        "params",
        "command",
        "echo",
    ] {
        assert!(
            !content.contains(forbidden),
            "notice leaked request material `{forbidden}`: {content}"
        );
    }

    // Destination re-derivation: the live authority agrees, and a stale or
    // caller-influenced authority is refused.
    let authority = f
        .base
        .store
        .approval_room_authority(f.base.identity.transport.engagement_id.clone())
        .await
        .unwrap();
    assert!(notice.matches(&authority));
    let mut stale = authority.clone();
    stale.project_room_id = "!caller-influenced:example.test".to_owned();
    assert!(!notice.matches(&stale), "stale destination must be refused");

    // The notice posts exactly once, to the project room, never the private.
    let mut posted = Vec::new();
    drive_with(
        f.collector
            .send_private_approval_notice(card, &CancellationToken::new()),
        &mut f.fake,
        &mut f.peer,
        |r, _, _| posted.push((r.method.clone(), r.target.clone())),
    )
    .await
    .unwrap();
    assert_eq!(posted.len(), 1, "notice sent exactly once");
    assert_eq!(posted[0].0, "PUT");
    assert!(
        posted[0].1.contains("/rooms/!project:example.test/send/"),
        "{}",
        posted[0].1
    );
    assert!(!posted[0].1.contains("!private"), "{}", posted[0].1);

    // Reading the notice confers no grant and no authority: the request is
    // still pending and no decision was written.
    let summary = f.base.store.approval_summary(request_id).await.unwrap();
    assert_eq!(summary.state, "pending");
    assert_eq!(summary.choice, None);

    f.close().await;
}

/// PC-C1 selector 2: a private send that fails denies the pending request
/// (D-PC-FC). The denial lands in the `owner_approvals` row every surface
/// serves, mints a kind-deny receipt carrying the named reason, and the
/// send is never retried.
#[tokio::test]
async fn native_private_approval_private_failure_denies_pending() {
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let card = f.card(1, true).await;
    let request_id = card.target().request_id.clone();

    // Refuse the private send: every room or to-device write fails. The pump
    // must not retry, and the failure must deny — never leave it pending.
    let mut sends = 0usize;
    let result = drive_with(
        f.collector
            .send_private_approval_card(card, &CancellationToken::new()),
        &mut f.fake,
        &mut f.peer,
        |r, _, reply| {
            if r.method == "PUT"
                && (r.target.contains("/send/") || r.target.contains("/sendToDevice/"))
            {
                sends += 1;
                reply.0 = 500;
            }
        },
    )
    .await;
    assert!(result.is_err(), "a failed send must surface as an error");
    assert_eq!(sends, 1, "a failed send is never retried");

    // The denial landed in the row every surface reads.
    let summary = f
        .base
        .store
        .approval_summary(request_id.clone())
        .await
        .unwrap();
    assert_eq!(summary.state, "decided");
    assert_eq!(summary.choice, Some(ApprovalChoice::Deny));

    // The named reason is minted on the kind-deny receipt row.
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
        reason.contains("failed"),
        "reason names the failed send: {reason}"
    );

    f.close().await;
}
