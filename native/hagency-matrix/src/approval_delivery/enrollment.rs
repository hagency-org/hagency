use super::jobs::Value;
use crate::{
    ApprovalCollector, CancellationToken, Error, collector::Inner, enrollment::Scope, sdk::Owner,
};
use std::collections::BTreeSet;
use tokio::time::Instant;
impl ApprovalCollector {
    /// Explicit fresh ordinary-user account enrollment for this approval purpose.
    /// No Agent transport, permission decision or card transmission is admitted.
    pub async fn enroll_fresh_account(&self, cancel: &CancellationToken) -> Result<(), Error> {
        if self.inner.config.enrollment.is_none() {
            return Err(Error::Config);
        }
        let permit = self.delivery_permit(false)?;
        let deadline = Instant::now() + self.inner.config.limits.sdk;
        let inner = self.inner.clone();
        let engagements = self.engagements.clone();
        let cancel = cancel.child_token();
        let job=self.jobs.start(false,false,permit,async move{
            let original_rooms=inner.approval_rooms(&engagements).await?;
            let work=async{
                inner.approval_enrollment_current(&engagements,&cancel).await?;
                let mut guard=inner.owner.lock().await;
                if guard.is_none(){*guard=Some(Owner::open(&inner.config).await?);}
                drop(guard);
                inner.enroll(Scope::Approval(&engagements),&cancel,deadline).await
            };
            tokio::pin!(work);
            let result=tokio::select!{r=&mut work=>r,_=tokio::time::sleep_until(deadline)=>{cancel.cancel();work.await}};
            match result {
                Ok(())=>Ok(Value::Unit),
                Err(error)=>match inner.fence_approval_candidates(&original_rooms).await {
                    Ok(())=>Err(error),Err(_)=>Err(Error::OutcomeUnknown),
                },
            }
        })?;
        match job.wait().await? {
            Value::Unit => Ok(()),
            _ => Err(Error::Storage),
        }
    }
}
impl Inner {
    pub(crate) async fn approval_enrollment_current(
        &self,
        engagements: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, Error> {
        let rooms = self.approval_rooms(engagements).await?;
        self.refresh_approval_rooms(&rooms, cancel).await?;
        // Re-read original domain authority after all observed/published snapshots.
        let current = self.approval_rooms(engagements).await?;
        if rooms.len() != current.len()
            || rooms.iter().zip(&current).any(|(a, b)| {
                a.authority != b.authority || a.device != b.device || a.generation != b.generation
            })
        {
            return Err(Error::Generation);
        }
        let mut users = BTreeSet::new();
        for room in &current {
            let capture = self
                .domain
                .approval_room_capture(room.authority.clone())
                .await?
                .ok_or(Error::Generation)?;
            if !capture.available
                || capture.device_id != room.device
                || capture.generation != room.generation
            {
                return Err(Error::Generation);
            }
            users.extend([
                room.authority.bot_mxid.clone(),
                room.authority.owner_mxid.clone(),
            ]);
        }
        let users: Vec<_> = users.into_iter().collect();
        self.config
            .enrollment
            .as_ref()
            .ok_or(Error::Config)?
            .users(&self.config.identity.transport.sender_mxid, &users)?;
        Ok(users)
    }
}
