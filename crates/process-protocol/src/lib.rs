//! Domain types shared by the process runtime and the inspection-only helper protocol.

mod containment;
mod helper;
mod run;
mod state;

pub use containment::{ContainmentGuarantee, ContainmentRequest};
pub use helper::{HelperOperation, HelperRequest, HelperResponse, HelperStatus};
pub use run::{DescendantPolicy, LaunchFact, RunId, RunSnapshot, RunSpec};
pub use state::{
    CleanupEvidence, CleanupState, DirectProcessState, ExitOutcome, ScopeState, StopRequest,
};
