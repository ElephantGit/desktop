use serde::{Deserialize, Serialize};

use crate::{
    AgentRef, ProjectId, ScheduledRunId, ScheduledTask, ScheduledTaskApproval, ScheduledTaskId,
    SessionId,
};

/// Identifies one planned trigger independently from the task revision that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledOccurrence {
    pub task_id: ScheduledTaskId,
    pub scheduled_at: i64,
}

/// Freezes the execution inputs selected when an occurrence is first admitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledRunSnapshot {
    pub revision: u64,
    pub project_id: ProjectId,
    pub agent_ref: AgentRef,
    pub model: Option<String>,
    pub prompt: String,
    pub approval: Option<ScheduledTaskApproval>,
}

/// Represents every lifecycle state defined for a persisted Scheduled Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledRunStatus {
    Accepted,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    SkippedBusy,
    SkippedCapacity,
}

/// Stores one admitted occurrence and the immutable inputs it will execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledRun {
    pub id: ScheduledRunId,
    pub occurrence: ScheduledOccurrence,
    pub snapshot: ScheduledRunSnapshot,
    pub status: ScheduledRunStatus,
    pub session_id: Option<SessionId>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ScheduledRun {
    /// Admits a planned occurrence by copying execution inputs out of the current definition.
    pub fn accepted(
        id: ScheduledRunId,
        occurrence: ScheduledOccurrence,
        task: &ScheduledTask,
        accepted_at: i64,
    ) -> Self {
        Self {
            id,
            occurrence,
            snapshot: ScheduledRunSnapshot {
                revision: task.revision,
                project_id: task.project_id.clone(),
                agent_ref: task.agent_ref.clone(),
                model: task.model.clone(),
                prompt: task.prompt.clone(),
                approval: task.approval.clone(),
            },
            status: ScheduledRunStatus::Accepted,
            session_id: None,
            created_at: accepted_at,
            updated_at: accepted_at,
        }
    }
}
