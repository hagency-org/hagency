//! Original owned callback coordination. Notices never carry verdict authority.
mod capacity;
mod control;
#[cfg(any(test, feature = "test-diagnostics"))]
pub mod diagnostics;
mod observations;
pub(crate) mod state;
pub use capacity::ApprovalHost;
pub(crate) use capacity::Reservation;
pub(crate) use observations::Drive;
pub(crate) use state::ApprovalRun;
use tokio::sync::mpsc;

pub struct ApprovalNotice {
    pub request_id: String,
    pub owner_expires_at: u64,
}
pub struct ApprovalRequests {
    receive: mpsc::Receiver<ApprovalNotice>,
}
impl ApprovalRequests {
    pub async fn recv(&mut self) -> Option<ApprovalNotice> {
        self.receive.recv().await
    }
}
pub(crate) fn notices() -> (mpsc::Sender<ApprovalNotice>, ApprovalRequests) {
    let (send, receive) = mpsc::channel(16);
    (send, ApprovalRequests { receive })
}

// Delivery-loss seams exist only in the library test build. Each follows an
// actual native or domain effect; none can manufacture a positive proof.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fault {
    RequestAck,
    ConsumeAck,
    BeginAck,
    WriteAck,
    /// Test seam: a *successful* acceptance call is converted to the reply-loss
    /// verdict, so the reconcile reads an error while the row is committed. It
    /// can only convert a success into an error; it can never manufacture an
    /// accepted row.
    WriteAckLost,
    SpawnPanic,
    WritePanic,
    BeginGate,
    RecheckGate,
    MaintainGate,
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct Gate {
    pub entered: std::sync::atomic::AtomicBool,
    pub release: std::sync::atomic::AtomicBool,
    pub usage_failed: std::sync::atomic::AtomicBool,
}
#[cfg(test)]
impl Gate {
    pub async fn wait(&self) {
        use std::sync::atomic::Ordering;
        self.entered.store(true, Ordering::Release);
        let until = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while !self.release.load(Ordering::Acquire) {
            assert!(
                tokio::time::Instant::now() < until,
                "bounded test receipt gate expired"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }
}
