#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
#[path = "../../hagency-matrix/tests/common/stall.rs"]
mod stall;
mod mcp_coordination {
    pub mod fixture;
    mod flows;
    mod recovery;
}
