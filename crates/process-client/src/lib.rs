//! Remote read-only guardian discovery; no runtime, database or process-launch dependency.

#[cfg(target_os = "linux")]
mod guardian;
#[cfg(target_os = "linux")]
pub use guardian::GuardianProbe;
