use super::*;
#[path = "../../../hagency/tests/fixtures/matrix_crypto_peer.rs"]
pub mod crypto;

pub struct Fixture {
    pub base: common::Fixture,
    pub collector: Collector,
    pub fake: common::Fake,
    pub peer: crypto::Peer,
}
impl Fixture {
    pub async fn new() -> Self {
        let base = common::Fixture::new();
        let peer = crypto::Peer::new().await;
        let mut fake = common::Fake::start(true).await;
        let collector = Collector::new(config(&base, &fake, &peer), base.store.clone()).unwrap();
        let (result, ()) = common::scripted(
            collector.collect(&CancellationToken::new()),
            common::success(&mut fake, "enroll-first"),
        )
        .await;
        result.unwrap();
        Self {
            base,
            collector,
            fake,
            peer,
        }
    }
    pub async fn run(&mut self) -> Result<(), Error> {
        drive(
            &self.collector,
            &mut self.fake,
            &mut self.peer,
            &CancellationToken::new(),
        )
        .await
    }
    pub async fn close(self) {
        self.collector.close().await.unwrap();
        common::shutdown_domain(&self.base.store, "enrollment-final").await;
        self.fake.close().await;
    }
}
pub fn config(
    base: &common::Fixture,
    fake: &common::Fake,
    peer: &crypto::Peer,
) -> crate::HostConfig {
    base.config(&fake.endpoint)
        .with_root_pem(include_bytes!("../fixtures/ca.pem"))
        .unwrap()
        .with_fresh_account_enrollment(vec![(crypto::HUMAN.into(), peer.anchor())])
        .unwrap()
}
pub async fn respond(request: common::Request, peer: &mut crypto::Peer) {
    respond_with(request, peer, &mut |_, _, _| {}).await;
}
pub async fn respond_with(
    request: common::Request,
    peer: &mut crypto::Peer,
    change: &mut impl FnMut(&common::Request, &mut crypto::Peer, &mut (u16, serde_json::Value)),
) {
    assert_eq!(
        request.headers.get("authorization"),
        Some(&format!("Bearer {}", common::TOKEN))
    );
    let mut reply = if request.target == "/_matrix/client/v3/account/whoami" {
        (200, common::who())
    } else if request.target.ends_with("/state") {
        (200, common::state())
    } else {
        let body = if request.body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&request.body).unwrap()
        };
        peer.protocol(&request.method, &request.target, &body)
            .await
            .expect("fixed enrollment protocol")
    };
    change(&request, peer, &mut reply);
    request.json(reply.0, reply.1);
}
pub async fn drive(
    collector: &Collector,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    drive_with(collector, fake, peer, cancel, |_, _, _| {}).await
}
pub async fn drive_with(
    collector: &Collector,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
    cancel: &CancellationToken,
    mut change: impl FnMut(&common::Request, &mut crypto::Peer, &mut (u16, serde_json::Value)),
) -> Result<(), Error> {
    let operation = collector.enroll_fresh_account(cancel);
    tokio::pin!(operation);
    let mut count = 0;
    loop {
        tokio::select! {
            result=&mut operation=>return result,
            request=fake.next()=>{count+=1;assert!(count<=96);respond_with(request,peer,&mut change).await;}
        }
    }
}

pub async fn stop_sdk(collector: &Collector) {
    if let Some(owner) = collector.inner.owner.lock().await.take() {
        owner.close().await.unwrap();
    }
}

pub async fn sdk_status(collector: &Collector) -> Result<crate::enrollment::state::View, Error> {
    let owner = crate::sdk::Owner::open_existing(&collector.inner.config).await?;
    let result = owner
        .enrollment(crate::sdk::enrollment::Command::Status)
        .await;
    owner.close().await.unwrap();
    result
}

pub async fn until_write<F: std::future::Future<Output = Result<(), Error>>>(
    operation: std::pin::Pin<&mut F>,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
    path: &str,
) -> common::Request {
    let mut operation = operation;
    for _ in 0..96 {
        tokio::select! {
            result=&mut operation=>panic!("original enrollment completed before held write: {result:?}"),
            request=fake.next()=>{
                if request.target == path {return request;}
                respond(request,peer).await;
            }
        }
    }
    panic!("original enrollment request bound exceeded")
}

pub async fn finish_owned(f: &mut Fixture) -> Result<(), Error> {
    let busy = f.collector.inner.busy.clone();
    let done = busy.acquire_owned();
    tokio::pin!(done);
    for _ in 0..96 {
        tokio::select! {
            permit=&mut done=>{
                drop(permit.unwrap());
                return f.collector.inner.enrollment_jobs.0.lock().unwrap().as_ref().unwrap()
                    .result.lock().unwrap().expect("original owned result retained");
            }
            request=f.fake.next()=>respond(request,&mut f.peer).await,
        }
    }
    panic!("original enrollment request bound exceeded")
}
