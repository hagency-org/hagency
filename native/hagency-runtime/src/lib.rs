//! Bounded runtime protocol and host-only owned child IO foundation. It grants
//! no permissions and cannot mutate canonical dispatch, task or Matrix state.
pub mod codex;
pub mod owned;
