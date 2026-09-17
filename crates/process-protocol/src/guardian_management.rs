use serde::{Deserialize, Serialize};

use crate::{
    GuardianChannel, GuardianCredential, GuardianReadyRequest, HostBinding, ScopeCreationIntent,
};

/// Additive message selection preserves the original readiness format on live older guardians.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GuardianRequest {
    Ready(GuardianReadyRequest),
    Management(GuardianManagementRequest),
}

/// A guardian-issued host session, separate from Node authorization and Scope control generations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardianHostSession {
    pub host: HostBinding,
    pub credential: GuardianCredential,
}

/// Only host binding changes are available; no variant grants workload mutation or lease renewal.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum GuardianManagementOperation {
    Bind { host: HostBinding },
    Inspect { session: GuardianHostSession },
}

/// Every channel authenticates the original scope before entering the serialized execution gate.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardianManagementRequest {
    pub version: u16,
    pub intent: ScopeCreationIntent,
    pub credential: GuardianCredential,
    pub channel: GuardianChannel,
    pub operation: GuardianManagementOperation,
}

/// Rejections do not disclose the current host or its session secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianManagementRejection {
    StaleHost,
    ConflictingHost,
    InvalidEpoch,
    WrongChannel,
    StaleSession,
    StorageUnavailable,
}

/// Successful binding follows its durable commit; inspection proves only current host-session status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum GuardianManagementReply {
    Bound {
        intent: ScopeCreationIntent,
        session: GuardianHostSession,
    },
    Current {
        intent: ScopeCreationIntent,
        session: GuardianHostSession,
        channel: GuardianChannel,
    },
    Rejected(GuardianManagementRejection),
}
