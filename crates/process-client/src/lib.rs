//! Guardian discovery and host binding; no runtime, database or process-launch dependency.

#[cfg(target_os = "linux")]
mod guardian;
#[cfg(target_os = "linux")]
pub use guardian::GuardianProbe;
#[cfg(target_os = "linux")]
mod management;
#[cfg(target_os = "linux")]
pub use management::GuardianManagement;
#[cfg(target_os = "linux")]
mod transport;
