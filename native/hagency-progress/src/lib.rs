//! Pure progress projection, not canonical task truth or transport authority.
mod accumulator;
mod policy;
mod summary;
mod tools;
pub use accumulator::*;
pub use policy::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
pub use summary::*;
pub use tools::*;

pub const MAX_EVENTS: usize = 1024;
pub const MAX_CALLS: usize = 256;
pub const MAX_ATTEMPTS: u32 = 256;
pub const MAX_COUNTER: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("progress configuration invalid: {0}")]
    Config(&'static str),
    #[error("progress input shape is invalid")]
    Shape,
    #[error("progress capacity exceeded")]
    Capacity,
    #[error("progress run identity mismatch")]
    Identity,
    #[error("progress observation replay changed")]
    Conflict,
    #[error("progress observation order is invalid")]
    Order,
    #[error("progress clock moved backwards")]
    Clock,
    #[error("progress run or attempt state is invalid")]
    State,
}

// Bound structure before hashing; retain only the hash, never raw titles/errors.
fn fingerprint(value: &Value, max: usize) -> Result<[u8; 32], Error> {
    let mut pending = vec![(value, 0)];
    let mut nodes = 0;
    while let Some((v, depth)) = pending.pop() {
        nodes += 1;
        let count = match v {
            Value::Array(a) => a.len(),
            Value::Object(o) => o.len(),
            _ => 0,
        };
        if depth > 8 || nodes + pending.len() + count > max {
            return Err(Error::Capacity);
        }
        match v {
            Value::Array(a) => pending.extend(a.iter().map(|v| (v, depth + 1))),
            Value::Object(o) => pending.extend(o.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    struct HashWriter {
        hash: Sha256,
        remaining: usize,
    }
    impl std::io::Write for HashWriter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.remaining = self
                .remaining
                .checked_sub(b.len())
                .ok_or_else(|| std::io::Error::other("capacity"))?;
            self.hash.update(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter {
        hash: Sha256::new(),
        remaining: max,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| Error::Capacity)?;
    Ok(writer.hash.finalize().into())
}
fn identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
