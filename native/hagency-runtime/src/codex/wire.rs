use super::{Error, MAX_FRAME_BYTES, PARTIAL_FRAME_MS, json, text};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::Write;

/// Signed integer and string are distinct protocol namespaces, including "0".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}
impl RequestId {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Number(_) => true,
            Self::String(value) => text(value, 256),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub data: Option<Value>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum Message {
    Request {
        id: RequestId,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        trace: Option<Value>,
    },
    Notification {
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
    Response {
        id: RequestId,
        result: Value,
    },
    Error {
        id: RequestId,
        error: RpcError,
    },
}

impl Message {
    fn parse(value: Value) -> Result<Self, Error> {
        let Value::Object(mut fields) = value else {
            return Err(Error::Envelope);
        };
        let id = fields
            .remove("id")
            .map(|value| serde_json::from_value::<RequestId>(value).map_err(|_| Error::Envelope))
            .transpose()?;
        if id.as_ref().is_some_and(|id| !id.valid()) {
            return Err(Error::Envelope);
        }
        let method = fields.remove("method");
        let message = if let Some(method) = method {
            let method = method.as_str().ok_or(Error::Envelope)?.to_owned();
            if !text(&method, 128) {
                return Err(Error::Envelope);
            }
            let params = fields.remove("params");
            if let Some(id) = id {
                let trace = fields.remove("trace");
                if let Some(trace) = &trace {
                    valid_trace(trace)?;
                }
                Self::Request {
                    id,
                    method,
                    params,
                    trace,
                }
            } else {
                Self::Notification { method, params }
            }
        } else {
            let id = id.ok_or(Error::Envelope)?;
            if let Some(result) = fields.remove("result") {
                Self::Response { id, result }
            } else {
                let error = fields.remove("error").ok_or(Error::Envelope)?;
                let error: RpcError = serde_json::from_value(error).map_err(|_| Error::Envelope)?;
                if error.message.len() > MAX_FRAME_BYTES {
                    return Err(Error::Capacity);
                }
                Self::Error { id, error }
            }
        };
        if !fields.is_empty() {
            return Err(Error::Envelope);
        }
        Ok(message)
    }
}

fn valid_trace(value: &Value) -> Result<(), Error> {
    if value.is_null() {
        return Ok(());
    }
    let fields = value.as_object().ok_or(Error::Envelope)?;
    if fields.iter().all(|(key, value)| {
        matches!(key.as_str(), "traceparent" | "tracestate")
            && (value.is_null() || value.as_str().is_some_and(|v| v.len() <= 1024))
    }) {
        Ok(())
    } else {
        Err(Error::Envelope)
    }
}

struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("frame capacity"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Encodes exactly one bounded line. The caller must confirm the entire write;
/// returning bytes alone is not child-stdin acceptance or a dispatch ACK.
pub fn encode(message: Message) -> Result<Vec<u8>, Error> {
    match &message {
        Message::Request { params, trace, .. } => {
            if let Some(value) = params {
                json::depth(value, 1)?;
            }
            if let Some(value) = trace {
                json::depth(value, 1)?;
            }
        }
        Message::Notification {
            params: Some(value),
            ..
        }
        | Message::Response { result: value, .. } => json::depth(value, 1)?,
        Message::Error { error, .. } => {
            if let Some(value) = &error.data {
                json::depth(value, 2)?;
            }
        }
        _ => (),
    }
    let mut out = Bounded(Vec::new());
    serde_json::to_writer(&mut out, &message).map_err(|_| Error::Capacity)?;
    Message::parse(json::parse(&out.0)?)?;
    out.0.push(b'\n');
    Ok(out.0)
}

/// Pull-style decoding never accumulates an unbounded vector of notifications.
/// Feed the unconsumed suffix again after processing the returned message.
#[derive(Default)]
pub struct Decoder {
    bytes: Vec<u8>,
    since: Option<u64>,
    clock: u64,
    closed: bool,
}

impl Decoder {
    pub(super) fn deadline_ms(&self) -> Option<u64> {
        self.since
            .map(|since| since.saturating_add(PARTIAL_FRAME_MS))
    }

    pub fn buffered_bytes(&self) -> usize {
        self.bytes.len()
    }

    fn fail<T>(&mut self, error: Error) -> Result<T, Error> {
        self.closed = true;
        self.bytes.clear();
        Err(error)
    }

    pub fn check_deadline(&mut self, now_ms: u64) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if now_ms < self.clock {
            return self.fail(Error::Clock);
        }
        self.clock = now_ms;
        if self.since.is_some_and(|at| now_ms - at >= PARTIAL_FRAME_MS) {
            return self.fail(Error::Timeout);
        }
        Ok(())
    }

    pub fn feed(&mut self, input: &[u8], now_ms: u64) -> Result<(usize, Option<Message>), Error> {
        self.check_deadline(now_ms)?;
        let remaining = MAX_FRAME_BYTES.saturating_sub(self.bytes.len());
        let end = input
            .iter()
            .take(remaining + 1)
            .position(|&byte| byte == b'\n');
        let length = end.unwrap_or(input.len());
        if length > MAX_FRAME_BYTES.saturating_sub(self.bytes.len()) {
            return self.fail(Error::Capacity);
        }
        if length > 0 && self.since.is_none() {
            self.since = Some(now_ms);
        }
        self.bytes.extend_from_slice(&input[..length]);
        let Some(end) = end else {
            return Ok((length, None));
        };
        let message = json::parse(&self.bytes).and_then(Message::parse);
        self.bytes.clear();
        self.since = None;
        match message {
            Ok(message) => Ok((end + 1, Some(message))),
            Err(error) => self.fail(error),
        }
    }

    pub fn eof(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let partial = !self.bytes.is_empty();
        self.closed = true;
        self.bytes.clear();
        if partial {
            Err(Error::UnexpectedEof)
        } else {
            Ok(())
        }
    }
}

pub(super) fn object() -> Value {
    Value::Object(Map::new())
}
