use super::*;
use hagency_store::{FileDeliveryIdentity, FilePublicationClaim, FilePublicationSend};

pub(super) async fn accepted(
    f: &mut Fixture,
    id: &str,
    caption: Option<&str>,
) -> Option<(UploadOperation, FileDeliveryIdentity, Vec<u8>)> {
    let (input, identity, ciphertext) = f.file_input(id, caption).await?;
    let mut original = f.admit(input);
    let cancel = CancellationToken::new();
    let (receipt, ()) =
        common::scripted(original.run(&cancel), post(&mut f.fake, &ciphertext)).await;
    assert_eq!(
        receipt.unwrap().upload,
        hagency_core::uploads::UploadState::Accepted
    );
    Some((original, identity, ciphertext))
}
pub(super) async fn publication(
    f: &Fixture,
    identity: FileDeliveryIdentity,
) -> (FilePublicationClaim, FilePublicationSend) {
    let claim = f
        .base
        .store
        .claim_file_publication(f.cap.clone(), identity, 60_000)
        .await
        .unwrap()
        .unwrap();
    let send = f
        .base
        .store
        .begin_file_publication(f.cap.clone(), claim.clone())
        .await
        .unwrap();
    (claim, send)
}
pub(super) async fn preflight(fake: &mut common::Fake) {
    let request = fake.next().await;
    assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
    request.json(200, common::who());
    let request = fake.next().await;
    assert!(request.target.ends_with("/state"));
    request.json(200, common::state());
}
pub(super) async fn wire(
    fake: &mut common::Fake,
    peer: &crate::sdk::outgoing_fixture::Peer,
    room: &ruma::RoomId,
) -> (common::Request, Value) {
    preflight(fake).await;
    let query = fake.next().await;
    assert_eq!(query.target, "/_matrix/client/v3/keys/query");
    query.json(200, peer.query.clone());
    preflight(fake).await;
    fake.next().await.json(200, peer.query.clone());
    let share = fake.next().await;
    assert!(share.target.contains("/sendToDevice/m.room.encrypted/"));
    peer.share(serde_json::from_slice(&share.body).unwrap())
        .await;
    share.json(200, json!({}));
    preflight(fake).await;
    fake.next().await.json(200, peer.query.clone());
    let message = fake.next().await;
    assert_eq!(message.method, "PUT");
    assert!(message.target.contains("/send/m.room.encrypted/"));
    let value: Value = serde_json::from_slice(&message.body).unwrap();
    assert!(value.get("body").is_none());
    let plain = peer.decrypt_in(value, room).await;
    (message, plain)
}
