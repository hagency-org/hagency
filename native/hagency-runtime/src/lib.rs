//! Bounded runtime protocol and host-only owned child IO foundation. It grants
//! no permissions and cannot mutate canonical dispatch, task or Matrix state.
pub mod claude;
pub mod codex;
mod json;
pub mod owned;
pub mod task_mcp;
