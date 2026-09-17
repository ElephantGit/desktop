//! Domain types shared by the process runtime and the inspection-only helper protocol.

mod containment;
mod guardian;
mod helper;
mod output;
mod run;
mod state;

pub use containment::{ContainmentGuarantee, ContainmentRequest};
pub use guardian::{
    GuardianInstanceId, HostBinding, HostInstanceId, InvalidProcessIdentity, ScopeCreationIntent,
    ScopeId,
};
pub use helper::{HelperOperation, HelperRequest, HelperResponse, HelperStatus};
pub use output::{OutputPolicy, OutputRead, OutputState, OutputStream};
pub use run::{DescendantPolicy, LaunchFact, RunId, RunSnapshot, RunSpec};
pub use state::{
    CleanupEvidence, CleanupState, DirectProcessState, ExitOutcome, ScopeState, StopRequest,
};
