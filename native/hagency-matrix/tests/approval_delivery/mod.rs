use super::*;
use crate::{
    HostApprovalConfig, HostApprovalPlan, HostConfig, HostIdentity, HostRoom,
    collector::fixtures as common,
};
use hagency_core::{approvals::*, replies::*, tasks::*};
use serde_json::{Value, json};
mod custody;
mod enrollment;
mod fixture;
mod journal;
mod privacy;
use fixture::*;
