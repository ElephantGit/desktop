//! Guardian lifecycle coordination for the new process runtime.

mod platform;
mod scope;
mod stop;

pub use platform::{
    ContainmentObservation, Platform, PlatformCapabilities, PlatformError, PlatformObservation,
    SpawnError, StopSignal,
};
pub use scope::{AdmissionError, ReconcileFailure, ScopeRuntime, StartError, StopError};
