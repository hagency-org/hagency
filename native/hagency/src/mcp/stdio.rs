use hagency::{
    mcp::{Error, FRAME_LIMIT, OUTPUT_LIMIT, Session},
    task_client::Context,
};
use std::{
    io::{BufRead, Write},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

struct WatchState {
    deadline: Option<Instant>,
    stopped: bool,
}
struct Watch {
    state: Arc<(Mutex<WatchState>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Watch {
    fn new() -> Result<Self, Error> {
        let state = Arc::new((
            Mutex::new(WatchState {
                deadline: None,
                stopped: false,
            }),
            Condvar::new(),
        ));
        let shared = state.clone();
        let worker = thread::Builder::new()
            .name("mcp-io-deadline".into())
            .spawn(move || {
                let (lock, wake) = &*shared;
                let mut state = lock.lock().unwrap();
                loop {
                    if state.stopped {
                        return;
                    }
                    if let Some(deadline) = state.deadline {
                        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                            // This is only the dedicated helper. Never called by the daemon.
                            // No stdout result, no rollback claim, no other PID is signalled.
                            std::process::exit(74);
                        };
                        state = wake.wait_timeout(state, left).unwrap().0;
                    } else {
                        state = wake.wait(state).unwrap();
                    }
                }
            })
            .map_err(|_| Error::Io)?;
        Ok(Self {
            state,
            worker: Some(worker),
        })
    }
    fn set(&self, duration: Option<Duration>) {
        let (lock, wake) = &*self.state;
        lock.lock().unwrap().deadline = duration.map(|v| Instant::now() + v);
        wake.notify_one();
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        lock.lock().unwrap().stopped = true;
        wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
/// Standalone executable boundary: synchronous stdio belongs to this process.
/// Do not invoke from a service: IO expiry terminates the current helper.
pub(super) fn run_stdio() -> Result<(), Error> {
    let context = Context::from_env().map_err(|_| Error::Context)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Io)?;
    let watch = Watch::new()?;
    let mut session = Session::new(context);
    let input = std::io::stdin();
    let output = std::io::stdout();
    let mut input = std::io::BufReader::with_capacity(4096, input.lock());
    let mut output = output.lock();
    let mut line = Vec::with_capacity(4096);
    loop {
        line.clear();
        watch.set(None);
        loop {
            let bytes = input.fill_buf().map_err(|_| Error::Io)?;
            if bytes.is_empty() {
                return if line.is_empty() {
                    Ok(())
                } else {
                    // EOF arrived mid-frame: the peer's write half closed
                    // between parts of one line. That is a transport/pipe
                    // fault, not a protocol refusal, and it is the only
                    // refusal that depends on timing rather than content.
                    Err(Error::Framing {
                        detail: "stdin reached EOF with a partial frame",
                        bound: FRAME_LIMIT,
                        observed: line.len(),
                    })
                };
            }
            if line.is_empty() {
                watch.set(Some(Duration::from_secs(10)));
            }
            let end = bytes.iter().position(|b| *b == b'\n');
            let count = end.unwrap_or(bytes.len());
            if line.len() + count > FRAME_LIMIT {
                return Err(Error::Framing {
                    detail: "frame exceeds FRAME_LIMIT",
                    bound: FRAME_LIMIT,
                    observed: line.len() + count,
                });
            }
            line.extend_from_slice(&bytes[..count]);
            input.consume(count + usize::from(end.is_some()));
            if end.is_some() {
                break;
            }
        }
        watch.set(None);
        if let Some(response) = runtime.block_on(session.handle(&line))? {
            let mut bytes = serde_json::to_vec(&response)
                .map_err(|_| Error::Protocol("response serialization failed"))?;
            if bytes.len() > OUTPUT_LIMIT {
                return Err(Error::Framing {
                    detail: "response exceeds OUTPUT_LIMIT",
                    bound: OUTPUT_LIMIT,
                    observed: bytes.len(),
                });
            }
            bytes.push(b'\n');
            watch.set(Some(Duration::from_secs(5)));
            output.write_all(&bytes).map_err(|_| Error::Io)?;
            output.flush().map_err(|_| Error::Io)?;
            watch.set(None);
        }
    }
}
