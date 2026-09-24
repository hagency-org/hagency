use super::{FileError, FileView, Registry, Setup, job::Job, pipeline, recovery};
use crate::bootstrap::Shared;
use hagency_media::{Codec, Limits};
use hagency_media_store::{Recovery, Store, SyncEvidence};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, atomic::Ordering},
};
use tokio::sync::{Semaphore, mpsc, oneshot};

pub(super) enum Command {
    Initialize(oneshot::Sender<Result<(), FileError>>),
    Submit(Arc<Job>, oneshot::Sender<Result<FileView, FileError>>),
    Close(oneshot::Sender<Result<(), FileError>>),
}
pub(super) fn run(
    shared: Shared,
    registry: Arc<Registry>,
    setup: Setup,
    commands: mpsc::Receiver<Command>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(value) => value,
        Err(_) => return,
    };
    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(serve(shared, registry, setup, commands)));
}
async fn serve(
    shared: Shared,
    registry: Arc<Registry>,
    setup: Setup,
    mut commands: mpsc::Receiver<Command>,
) {
    let media: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let codec = Codec::new(Limits::new(setup.limit, 2).expect("validated fixed codec bound"));
    let gate = Rc::new(Semaphore::new(1));
    let mut tasks = tokio::task::JoinSet::new();
    // The same two registry slots bound these task-to-original associations.
    let mut task_jobs = BTreeMap::new();
    let mut initialized = None;
    let mut panic_seen = false;
    let mut close_reply = None;
    loop {
        tokio::select! {
            command=commands.recv()=> match command {
                Some(Command::Initialize(reply))=>{
                    let result=if registry.closed.load(Ordering::Acquire){Err(FileError::Unavailable)}else if let Some(result)=initialized{result}else{
                        match recovery::open(&setup) {
                            Ok(store)=>{
                                let result=if store.recovery()!=Recovery::Clean {Err(FileError::Unknown)}else if store.sync_evidence()!=SyncEvidence::FileAndDirectorySynced {Err(FileError::Unavailable)}else{Ok(())};
                                *media.borrow_mut()=Some(store);registry.ready.store(result.is_ok(),Ordering::Release);result
                            },
                            Err(error)=>Err(error),
                        }
                    };
                    initialized=Some(result);let _=reply.send(result);
                },
                Some(Command::Submit(job,reply))=>{
                    // Registry slots already bound this set. No extra jobs or subscribers.
                    let (shared,registry,media,codec,gate)=(shared.clone(),registry.clone(),media.clone(),codec.clone(),gate.clone());
                    let limit=setup.limit;
                    let original=job.clone();
                    let task=tasks.spawn_local(async move {pipeline::job(pipeline::Context{shared,registry,media,codec,gate,limit},job.clone(),reply).await;job});
                    task_jobs.insert(task.id(),original);
                },
                Some(Command::Close(reply))=>{
                    registry.closed.store(true,Ordering::Release);registry.cancel.cancel();
                    if close_reply.is_some(){let _=reply.send(Err(FileError::Unknown));}else{close_reply=Some(reply);}
                },
                None=>{
                    registry.closed.store(true,Ordering::Release);registry.cancel.cancel();
                    if registry.empty()&&tasks.is_empty(){return;}
                    // Abrupt owner loss cannot drop original unresolved objects or imply no write.
                    std::future::pending::<()>().await;
                },
            },
            result=tasks.join_next_with_id(),if !tasks.is_empty()=>{
                match result {
                    Some(Ok((id,job)))=>{
                        if task_jobs.remove(&id).is_some_and(|original|Arc::ptr_eq(&original,&job)) {
                            if job.info.lock().is_ok_and(|i|i.releasable)&&registry.remove(&job).is_err(){
                                panic_seen=true;registry.ready.store(false,Ordering::Release);job.mark_unknown();
                            }
                        }else{
                            panic_seen=true;registry.ready.store(false,Ordering::Release);registry.mark_unknown();
                        }
                    },
                    Some(Err(error))=>{
                        if let Some(original)=task_jobs.remove(&error.id()) {original.mark_unknown();}else{registry.mark_unknown();}
                        panic_seen=true;registry.ready.store(false,Ordering::Release);
                    },
                    None=>{},
                }
            }
        }
        if close_reply.is_some() {
            let unknown = panic_seen
                || initialized == Some(Err(FileError::Unknown))
                || registry.jobs.lock().map_or(true, |jobs| {
                    jobs.values()
                        .any(|j| j.info.lock().map_or(true, |i| !i.live && !i.releasable))
                });
            if unknown {
                if let Some(reply) = close_reply.take() {
                    let _ = reply.send(Err(FileError::Unknown));
                }
            } else if registry.empty() && tasks.is_empty() {
                drop(media.borrow_mut().take());
                #[cfg(test)]
                if registry.tests.close_mode.load(Ordering::Acquire) == 1 {
                    drop(close_reply.take());
                    return;
                }
                if let Some(reply) = close_reply.take() {
                    let _ = reply.send(Ok(()));
                }
                #[cfg(test)]
                assert_ne!(
                    registry.tests.close_mode.load(Ordering::Acquire),
                    2,
                    "fixture worker unwind after close ACK"
                );
                return;
            }
        }
    }
}
