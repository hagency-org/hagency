use super::{Error, Event, MAX_EVENT_BYTES, MAX_EVENTS, RequestId, STDERR_BYTES, StderrSnapshot};
use serde_json::Value;
use std::{collections::VecDeque, io::Write};

#[derive(Default)]
pub(super) struct PrivateStderr {
    tail: VecDeque<u8>,
    total: u64,
}
impl PrivateStderr {
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.total = self
            .total
            .checked_add(bytes.len() as u64)
            .ok_or(Error::Capacity)?;
        let drop = self
            .tail
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(STDERR_BYTES);
        self.tail.drain(..drop.min(self.tail.len()));
        self.tail.extend(
            bytes
                .iter()
                .skip(bytes.len().saturating_sub(STDERR_BYTES))
                .copied(),
        );
        Ok(())
    }
    pub fn snapshot(&self) -> StderrSnapshot {
        StderrSnapshot {
            total_bytes: self.total,
            tail: self.tail.iter().copied().collect(),
        }
    }
}

struct Count(usize);
impl Write for Count {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|&n| n <= MAX_EVENT_BYTES)
            .ok_or_else(|| std::io::Error::other("event bytes"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn json_bytes(value: &Value) -> Result<usize, Error> {
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value).map_err(|_| Error::Capacity)?;
    Ok(count.0)
}
fn id_bytes(id: &RequestId) -> usize {
    match id {
        RequestId::Number(_) => 20,
        RequestId::String(id) => id.len(),
    }
}
pub(super) fn charge(event: &Event) -> Result<usize, Error> {
    let data = match event {
        Event::Initialized { result } => json_bytes(result)?,
        Event::Response { id, method, result } => {
            id_bytes(id)
                + method.len()
                + match result {
                    Ok(value) => json_bytes(value)?,
                    Err(error) => {
                        20 + error.message.len()
                            + error
                                .data
                                .as_ref()
                                .map(json_bytes)
                                .transpose()?
                                .unwrap_or(0)
                    }
                }
        }
        Event::Notification { method, params } => {
            method.len() + params.as_ref().map(json_bytes).transpose()?.unwrap_or(0)
        }
        Event::ServerRequest { id, method, params } => {
            id_bytes(id) + method.len() + params.as_ref().map(json_bytes).transpose()?.unwrap_or(0)
        }
        Event::InterruptAcknowledged { scope } => scope.thread_id().len() + scope.turn_id().len(),
        Event::InterruptRejected { scope, error } => {
            scope.thread_id().len()
                + scope.turn_id().len()
                + 20
                + error.message.len()
                + error
                    .data
                    .as_ref()
                    .map(json_bytes)
                    .transpose()?
                    .unwrap_or(0)
        }
    };
    data.checked_add(std::mem::size_of::<Event>())
        .ok_or(Error::Capacity)
}

#[derive(Default)]
pub(super) struct EventQueue {
    events: VecDeque<(Event, usize)>,
    bytes: usize,
}
impl EventQueue {
    pub fn len(&self) -> usize {
        self.events.len()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn push(&mut self, event: Event) -> Result<(), Error> {
        let charge = charge(&event)?;
        let total = self.bytes.checked_add(charge).ok_or(Error::Capacity)?;
        if self.events.len() >= MAX_EVENTS || total > MAX_EVENT_BYTES {
            return Err(Error::Capacity);
        }
        self.events.push_back((event, charge));
        self.bytes = total;
        Ok(())
    }
    pub fn pop(&mut self) -> Option<Event> {
        let (event, charge) = self.events.pop_front()?;
        self.bytes -= charge;
        Some(event)
    }
    pub fn clear(&mut self) {
        self.events.clear();
        self.bytes = 0;
    }
}
