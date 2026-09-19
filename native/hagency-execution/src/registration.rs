//! Local ordering only: a registered workspace does not create domain authority.
use crate::{Failure, StartedWorkspace};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("workspace registration receiver is no longer available")]
pub struct RegistrationError;

/// One exact post-Started host handoff. Never decoded from runtime input.
pub struct WorkspaceRegistration {
    binding: StartedWorkspace,
    ack: LaunchAck,
}
impl WorkspaceRegistration {
    pub fn into_parts(self) -> (StartedWorkspace, LaunchAck) {
        (self.binding, self.ack)
    }
}
/// Consume only after the host's finite binding slot has accepted registration.
/// The worker independently checks cancellation, deadline and current authority.
pub struct LaunchAck(oneshot::Sender<()>);
impl LaunchAck {
    pub fn registered(self) -> Result<(), RegistrationError> {
        self.0.send(()).map_err(|_| RegistrationError)
    }
}
pub(crate) type RegistrationSlot = Arc<Mutex<Option<WorkspaceRegistration>>>;
pub(crate) struct Gate {
    slot: RegistrationSlot,
    sender: oneshot::Sender<()>,
    receiver: oneshot::Receiver<()>,
}
impl Gate {
    pub(crate) fn new() -> (Self, RegistrationSlot) {
        let slot = Arc::new(Mutex::new(None));
        let (sender, receiver) = oneshot::channel();
        (
            Self {
                slot: slot.clone(),
                sender,
                receiver,
            },
            slot,
        )
    }
    pub(crate) fn publish(
        self,
        binding: StartedWorkspace,
    ) -> Result<oneshot::Receiver<()>, Failure> {
        let mut slot = self.slot.lock().map_err(|_| Failure::Worker)?;
        if slot.is_some() {
            return Err(Failure::Worker);
        }
        *slot = Some(WorkspaceRegistration {
            binding,
            ack: LaunchAck(self.sender),
        });
        Ok(self.receiver)
    }
}
