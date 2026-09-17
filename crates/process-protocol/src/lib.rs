//! Domain types and bounded wire messages for process runtime, guardian discovery and helper inspection.

mod containment;
mod guardian;
mod guardian_wire;
pub use guardian_wire::{
    GUARDIAN_MAX_FRAME, GUARDIAN_WIRE_VERSION, GuardianAccess, GuardianBootstrap, GuardianChannel,
    GuardianCredential, GuardianReady, GuardianReadyRequest, decode_guardian_payload,
    encode_guardian_frame,
};
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
