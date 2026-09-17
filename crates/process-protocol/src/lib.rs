//! Domain types shared by the new process runtime. Wire encoding is not yet defined.

mod containment;
mod run;
mod state;

pub use containment::{ContainmentGuarantee, ContainmentRequest};
pub use run::{DescendantPolicy, LaunchFact, RunId, RunSnapshot, RunSpec};
pub use state::{
    CleanupEvidence, CleanupState, DirectProcessState, ExitOutcome, ScopeState, StopRequest,
};
