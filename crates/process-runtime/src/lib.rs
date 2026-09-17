//! Guardian lifecycle coordination for the new process runtime.

#[cfg(target_os = "linux")]
mod linux_best_effort;
#[cfg(target_os = "linux")]
mod linux_helper;
#[cfg(target_os = "linux")]
pub use linux_best_effort::LinuxBestEffort;
mod platform;
mod scope;
mod stop;
#[cfg(target_os = "linux")]
pub use linux_helper::{
    LinuxHelperConfig, check_linux_helper_deployment, serve_linux_helper,
    spawn_linux_helper_workload,
};

pub use platform::{
    ContainmentObservation, Platform, PlatformCapabilities, PlatformError, PlatformObservation,
    SpawnError, StopSignal,
};
pub use scope::{AdmissionError, ReconcileFailure, ScopeRuntime, StartError, StopError};
