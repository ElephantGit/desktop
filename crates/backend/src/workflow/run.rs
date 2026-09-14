//! Backend composition and runtime adapters for workflow runs.

mod api;
mod checkpoint;
mod engine;
mod executor;
pub(crate) mod interactive;
mod operations;
mod prerequisites;
mod prompt;
mod recovery;
mod rollback;
mod session_mcp;
pub(crate) use session_mcp::WorkflowSessionMcpSelectionSource;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod rollback_tests;
#[cfg(test)]
mod test_fixture;
mod worktree;

pub(crate) use engine::build_workflow_run_engine;
pub(crate) use operations::WorkflowRunSetup;
pub use operations::WorkflowRuns;
pub(crate) use recovery::{prune_orphaned_baselines, run_workflow_run_boot_sweep};
