//! The workflow run execution engine.
//!
//! Owns frozen-graph parsing and topology, the engine persistence port, and the run engine.
//! Agent-node execution is delegated through the `NodeExecutor` port (implemented in the backend)
//! and persistence through `WorkflowRunEngineRepository` (implemented in the database layer).

// The design places the run engine in `engine/engine.rs`, so the nested module name matches the
// containing directory on purpose.
mod agent_config;
mod branch_projection;
mod condition;
#[allow(clippy::module_inception)]
mod engine;
mod failure;
mod graph;
mod handlers;
mod node_type;
mod ports;
mod skill_delivery;
mod snapshot_switch;
mod structured_output;
mod variable_pool;
mod variable_template;
mod variable_value;

pub use agent_config::AgentMcp;
pub use engine::{
    EngineError, NodeExecutor, WorkflowRunCallback, WorkflowRunEngine, WorkflowValidationError,
};
pub use failure::{NodeFailure, NodeFailureDetail, NodeFailureKind};
pub use graph::{
    AgentConfig, AgentExecutor, AgentOutputContract, AgentSkill, GraphError,
    StructuredTextExposure, WorkflowGraph, WorkflowGraphNode,
};
pub use handlers::WorkflowRunControlHandler;
pub use node_type::{NodeType, UnknownNodeType};
pub use ports::{
    AdvanceWorkflowRunResult, BindWorkflowNodeSessionResult, CancelWorkflowRunResult,
    ExecutionContext, FileChange, NodeRunToStart, RestartWorkflowRunResult,
    ResumeWorkflowRunResult, StartPrerequisitesError, StartWorkflowRunResult,
    UpdateWorkflowRunInputResult, WorkflowNodeRunIdGenerator, WorkflowRunEngineRepository,
    WorkflowRunWorkspaceInitializer,
};
pub use skill_delivery::{
    AgentSkillDelivery, AgentSkillDeliveryError, AgentSkillDeliveryProvider,
    MaterializedSkillBinding, SkillDiscoveryRoots, SkillMaterializationReceipt, WorkflowRunPayload,
};
pub use snapshot_switch::{SnapshotIncompatibility, SnapshotSwitchPlan, plan_snapshot_switch};
pub use structured_output::{StructuredOutputError, extract_json_object, validate_against_schema};
pub use variable_pool::WorkflowVariablePool;
pub use variable_template::{VariableTemplateError, render_variable_template};

#[cfg(test)]
mod d2_tests;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod tests;
