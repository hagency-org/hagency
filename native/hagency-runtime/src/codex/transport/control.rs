//! Cooperative control at cancel-safe leaf IO, never around a public read.
use super::*;
use std::{future::Future, pin::Pin};

pub(in crate::codex) enum Controlled<T> {
    Event(Event),
    Control(T),
}

/// The original encoded bytes move between this value and the sole writer.
/// Neither this value nor its bytes has a public reconstruction/clone path.
pub(in crate::codex) struct PreparedFrame {
    id: RequestId,
    bytes: Option<Vec<u8>>,
    deadline: Instant,
    write_deadline: Option<Instant>,
}

impl<R, W, E> Driver<R, W, E> {
    pub(in crate::codex) fn event_deadline(&self) -> Instant {
        Instant::now() + Duration::from_millis(self.limits.event_wait_ms)
    }
    pub(in crate::codex) fn control_policy_fits(&self, owner_ms: u64, reserve_ms: u64) -> bool {
        owner_ms != 0
            && reserve_ms >= self.limits.write_timeout_ms
            && owner_ms.checked_add(reserve_ms).is_some_and(|total| {
                total <= MAX_REQUEST_MS
                    && Instant::now() + Duration::from_millis(total) <= self.lifetime
            })
    }
    pub(in crate::codex) fn approval_deadlines(
        &self,
        id: &RequestId,
        owner_ms: u64,
        reserve_ms: u64,
    ) -> Result<(Instant, Instant), Error> {
        let admitted = self
            .connection
            .approval_admitted_ms(id)
            .ok_or(Error::Closed)?;
        let owner = self.origin + Duration::from_millis(admitted + owner_ms);
        let response = owner + Duration::from_millis(reserve_ms);
        if owner <= Instant::now() || response > self.lifetime {
            return Err(Error::Timeout);
        }
        Ok((owner, response))
    }
    pub(in crate::codex) fn begin_prepared_send(
        &mut self,
        prepared: &mut PreparedFrame,
    ) -> Result<(), Error> {
        let deadline = *prepared.write_deadline.get_or_insert_with(|| {
            prepared
                .deadline
                .min(Instant::now() + Duration::from_millis(self.limits.write_timeout_ms))
        });
        self.check(deadline)
    }
    pub(in crate::codex) fn prepare_approval(
        &mut self,
        response: crate::codex::approval::ApprovalResponse,
        deadline: Instant,
    ) -> Result<PreparedFrame, Error> {
        self.check(deadline)?;
        let id = response.request.id().clone();
        let bytes = self.connection.respond_approval(response, self.now_ms())?;
        Ok(PreparedFrame {
            id,
            bytes: Some(bytes),
            deadline,
            write_deadline: None,
        })
    }
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin> Driver<R, W, E> {
    pub(in crate::codex) async fn next_or_control<F: Future + ?Sized>(
        &mut self,
        mut control: Pin<&mut F>,
        deadline: Instant,
    ) -> Result<Controlled<F::Output>, Error> {
        let operation = Operation {
            driver: self,
            finished: false,
        };
        let result = operation
            .driver
            .control_inner(control.as_mut(), deadline)
            .await;
        operation.finish(result)
    }

    async fn control_inner<F: Future + ?Sized>(
        &mut self,
        mut control: Pin<&mut F>,
        deadline: Instant,
    ) -> Result<Controlled<F::Output>, Error> {
        loop {
            tokio::task::yield_now().await;
            self.check(deadline)?;
            if let Some(event) = self.buffered_event()? {
                return Ok(Controlled::Event(event));
            }
            let wake = self.next_deadline(deadline);
            let streams = self.streams.as_mut().ok_or(Error::Closed)?;
            // Only the borrowed control future and cancellation-safe single reads
            // are selected. Accepted bytes are stored before the next await.
            tokio::select! {
                biased;
                output = control.as_mut() => {
                    self.check(deadline)?;
                    return Ok(Controlled::Control(output));
                }
                _ = tokio::time::sleep_until(wake) => return Err(Error::Timeout),
                result = streams.stdout.read(&mut self.input) => {
                    self.input_end = result.map_err(|_| Error::Io)?;
                    self.input_start = 0;
                    if self.input_end == 0 { return Err(Error::PeerEof); }
                }
                result = streams.stderr.read(&mut self.stderr_buffer), if self.stderr_open => {
                    let n = result.map_err(|_| Error::Io)?;
                    if n == 0 { self.stderr_open = false; }
                    else { self.diagnostics.append(&self.stderr_buffer[..n])?; }
                }
            }
            self.check(deadline)?;
        }
    }

    pub(in crate::codex) async fn send_prepared_or_event(
        &mut self,
        prepared: &mut PreparedFrame,
    ) -> Result<Controlled<TransportWrite>, Error> {
        let operation = Operation {
            driver: self,
            finished: false,
        };
        let result = operation.driver.prepared_inner(prepared).await;
        operation.finish(result)
    }

    async fn prepared_inner(
        &mut self,
        prepared: &mut PreparedFrame,
    ) -> Result<Controlled<TransportWrite>, Error> {
        // This deadline is installed once, even if a received update returns
        // before the first byte. Continuation cannot restart the write clock.
        let deadline = *prepared.write_deadline.get_or_insert_with(|| {
            prepared
                .deadline
                .min(Instant::now() + Duration::from_millis(self.limits.write_timeout_ms))
        });
        self.check(deadline)?;
        if self.writing.is_some() {
            return Err(Error::Closed);
        }
        self.writing = Some(Writing {
            id: Some(prepared.id.clone()),
            bytes: prepared.bytes.take().ok_or(Error::Closed)?,
            offset: 0,
            flushed: false,
        });
        loop {
            tokio::task::yield_now().await;
            self.check(deadline)?;
            let writing = self.writing.as_ref().ok_or(Error::Closed)?;
            if writing.flushed {
                let writing = self.writing.take().ok_or(Error::Closed)?;
                return Ok(Controlled::Control(TransportWrite {
                    request_id: writing.id,
                    bytes: writing.bytes.len(),
                }));
            }
            if writing.offset != 0 {
                // Once any bytes are accepted, finish/flush the same frame or
                // fail with original accepted-byte evidence. Never retry it.
                self.step(deadline).await?;
                continue;
            }
            if let Some(event) = self.buffered_event()? {
                // Exact Vec custody returns to the original prepared value;
                // encoding, callback reservation and deadline are not repeated.
                prepared.bytes = Some(self.writing.take().ok_or(Error::Closed)?.bytes);
                return Ok(Controlled::Event(event));
            }
            if !self.connection.has_prepared_approval(&prepared.id) {
                return Err(Error::Closed);
            }
            self.first_write_step(deadline).await?;
        }
    }

    async fn first_write_step(&mut self, deadline: Instant) -> Result<(), Error> {
        let partial = self.has_partial_frame();
        let wake = self.next_deadline(deadline);
        let streams = self.streams.as_mut().ok_or(Error::Closed)?;
        let writing = self.writing.as_mut().ok_or(Error::Closed)?;
        // Read currently ready stdout before polling the first write. A partial
        // frame must finish and be delivered first. No keepalive is fabricated.
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(wake) => return Err(Error::Timeout),
            result = streams.stdout.read(&mut self.input) => {
                self.input_end = result.map_err(|_| Error::Io)?;
                self.input_start = 0;
                if self.input_end == 0 { return Err(Error::PeerEof); }
            }
            result = streams.stdin.write(&writing.bytes), if !partial => {
                let n = result.map_err(|_| Error::Io)?;
                if n == 0 { return Err(Error::PeerEof); }
                if n > writing.bytes.len() { return Err(Error::Io); }
                writing.offset = n;
            }
            result = streams.stderr.read(&mut self.stderr_buffer), if self.stderr_open => {
                let n = result.map_err(|_| Error::Io)?;
                if n == 0 { self.stderr_open = false; }
                else { self.diagnostics.append(&self.stderr_buffer[..n])?; }
            }
        }
        self.check(deadline)
    }
}
