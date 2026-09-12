use super::{ReceiveError, ReceiveView, Registry, Setup, job::Job, pipeline, recovery};
use crate::bootstrap::Shared;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};
use tokio::{
    sync::{Semaphore, mpsc, oneshot},
    time::Instant,
};
pub(super) enum Command {
    Submit(Arc<Job>),
    Read {
        job: Arc<Job>,
        deadline: Instant,
        replayed: bool,
        reply: oneshot::Sender<Result<ReceiveView, ReceiveError>>,
    },
    Close(oneshot::Sender<Result<(), ReceiveError>>),
}
pub(super) fn run(
    runtime: tokio::runtime::Runtime,
    shared: Shared,
    registry: Arc<Registry>,
    setup: Setup,
    commands: mpsc::Receiver<Command>,
) {
    let local = tokio::task::LocalSet::new();
    // The registry lives outside this catch boundary, so a worker-level unwind
    // cannot destroy the actual retained jobs or their partial destinations.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(local.run_until(serve(shared.clone(), registry.clone(), setup, commands)))
    }));
    if result.is_err() {
        registry.unknown();
        // Keep this original worker's registry and shared Collector even if all
        // request handles disappear. A failed owner cannot acknowledge close.
        loop {
            std::thread::park();
        }
    }
}
async fn serve(
    shared: Shared,
    registry: Arc<Registry>,
    setup: Setup,
    mut commands: mpsc::Receiver<Command>,
) {
    let gate = Arc::new(Semaphore::new(1));
    let mut tasks = tokio::task::JoinSet::new();
    let mut task_jobs = BTreeMap::new();
    let mut close_reply = None;
    let mut channel_open = true;
    let mut panic_seen = false;
    loop {
        tokio::select! {
            command=commands.recv(),if channel_open=>match command {
                Some(Command::Submit(job))=>{
                    let (shared,registry,gate)=(shared.clone(),registry.clone(),gate.clone());
                    let original=job.clone();let limit=setup.limit;
                    let task=tasks.spawn_local(async move {pipeline::run(shared,registry,gate,job.clone(),limit).await;job});
                    task_jobs.insert(task.id(),original);
                },
                Some(Command::Read{job,deadline,replayed,reply})=>{
                    let result=tokio::select! {
                        biased;
                        _=registry.cancel.cancelled()=>Err(ReceiveError::Unauthorized),
                        _=tokio::time::sleep_until(deadline)=>Err(ReceiveError::Unavailable),
                        permit=gate.acquire()=>match permit {
                            Ok(_permit)=>recovery::read(&shared,&registry,&job,deadline,replayed).await,
                            Err(_)=>Err(ReceiveError::Unknown),
                        },
                    };
                    let _=reply.send(result);
                },
                Some(Command::Close(reply))=>{
                    registry.closed.store(true,Ordering::Release);registry.cancel.cancel();
                    if close_reply.is_some(){let _=reply.send(Err(ReceiveError::Unknown));}else{close_reply=Some(reply);}
                },
                None=>{
                    channel_open=false;registry.closed.store(true,Ordering::Release);registry.cancel.cancel();
                },
            },
            result=tasks.join_next_with_id(),if !tasks.is_empty()=>match result {
                Some(Ok((id,job)))=>{
                    if task_jobs.remove(&id).is_some_and(|old|Arc::ptr_eq(&old,&job)) && registry.joined(&job).is_ok(){job.acknowledge();}
                    else{job.unknown();job.acknowledge();registry.unknown();panic_seen=true;}
                },
                Some(Err(error))=>{
                    if let Some(original)=task_jobs.remove(&error.id()){recovery::lost_task(&shared,&original).await;}
                    else{registry.unknown();}
                    panic_seen=true;registry.closed.store(true,Ordering::Release);registry.cancel.cancel();
                },
                None=>{},
            },
            else=>std::future::pending::<()>().await,
        }
        if (close_reply.is_some() || !channel_open) && tasks.is_empty() {
            let mut entries = registry.entries.lock().unwrap_or_else(|e| e.into_inner());
            if panic_seen || !entries.live.is_empty() {
                if let Some(reply) = close_reply.take() {
                    let _ = reply.send(Err(ReceiveError::Unknown));
                }
                // A closed channel is never polled again. The original registry
                // remains retained here; no unknown file is deleted or replaced.
            } else {
                entries.ready.clear();
                if let Some(reply) = close_reply.take() {
                    let _ = reply.send(Ok(()));
                }
                return;
            }
        }
    }
}
