use serde::{Deserialize, Serialize};

use crate::{
    AgentRef, ProjectId, ScheduledRunId, ScheduledTask, ScheduledTaskApproval, ScheduledTaskId,
    SessionId,
};

/// Allows a live scheduler observation to arrive no more than ten minutes late.
pub const MAX_SCHEDULED_OCCURRENCE_LATENESS_MILLIS: i64 = 10 * 60 * 1_000;

/// Caps simultaneous local Scheduled Runs in the first release.
pub const MAX_CONCURRENT_LOCAL_SCHEDULED_RUNS: i64 = 4;

/// Bounds persisted run and missed history for one Scheduled Task.
pub const MAX_SCHEDULED_TASK_HISTORY_ENTRIES: i64 = 100;

/// Identifies one planned trigger independently from the task revision that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledOccurrence {
    pub task_id: ScheduledTaskId,
    pub scheduled_at: i64,
}

impl ScheduledOccurrence {
    /// Classifies a live scheduler observation without creating a persisted fact early.
    pub fn timing_at(&self, observed_at: i64) -> ScheduledOccurrenceTiming {
        if observed_at < self.scheduled_at {
            return ScheduledOccurrenceTiming::NotDue;
        }
        if observed_at.saturating_sub(self.scheduled_at) <= MAX_SCHEDULED_OCCURRENCE_LATENESS_MILLIS
        {
            return ScheduledOccurrenceTiming::Due;
        }
        ScheduledOccurrenceTiming::Missed(ScheduledMissReason::LateTrigger)
    }
}

/// Describes whether a live scheduled time may be admitted at the observation instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledOccurrenceTiming {
    NotDue,
    Due,
    Missed(ScheduledMissReason),
}

/// Records why a planned occurrence was observed without creating a Scheduled Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledMissReason {
    AppNotRunning,
    LateTrigger,
}

/// Stores a task-level missed fact rather than fabricating a failed Scheduled Run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledMissedOccurrence {
    pub occurrence: ScheduledOccurrence,
    pub reason: ScheduledMissReason,
    pub observed_at: i64,
}

/// Presents one persisted run or missed fact in task history order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduledTaskHistoryEntry {
    Run(ScheduledRun),
    Missed(ScheduledMissedOccurrence),
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

impl ScheduledRunStatus {
    /// Reports whether this state occupies the task and global admission slots.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Accepted | Self::Running)
    }
}

/// Records the stable reason a Scheduled Run could not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledRunFailureReason {
    TargetUnavailable,
    ModelUnavailable,
    WorktreeFailed,
    ApprovalPolicyInvalid,
    Timeout,
    ExecutionError,
}

/// Names one forward-only lifecycle change requested by the local execution adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledRunTransition {
    Start {
        session_id: SessionId,
        worktree_path: String,
    },
    Succeed,
    Fail {
        reason: ScheduledRunFailureReason,
    },
    Cancel,
}

impl ScheduledRunTransition {
    /// Rejects duplicate, backwards, and terminal-state transitions.
    pub fn is_allowed_from(&self, status: ScheduledRunStatus) -> bool {
        match self {
            Self::Start { .. } => status == ScheduledRunStatus::Accepted,
            Self::Succeed => status == ScheduledRunStatus::Running,
            Self::Fail { .. } | Self::Cancel => status.is_active(),
        }
    }
}

/// Stores one admitted occurrence and the immutable inputs it will execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledRun {
    pub id: ScheduledRunId,
    pub occurrence: ScheduledOccurrence,
    pub snapshot: ScheduledRunSnapshot,
    pub status: ScheduledRunStatus,
    pub session_id: Option<SessionId>,
    pub worktree_path: Option<String>,
    pub failure_reason: Option<ScheduledRunFailureReason>,
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
            worktree_path: None,
            failure_reason: None,
            created_at: accepted_at,
            updated_at: accepted_at,
        }
    }

    /// Applies one valid forward transition while preserving execution references for history.
    pub fn transition(&mut self, transition: ScheduledRunTransition, transitioned_at: i64) -> bool {
        if !transition.is_allowed_from(self.status) {
            return false;
        }

        match transition {
            ScheduledRunTransition::Start {
                session_id,
                worktree_path,
            } => {
                self.status = ScheduledRunStatus::Running;
                self.session_id = Some(session_id);
                self.worktree_path = Some(worktree_path);
                self.failure_reason = None;
            }
            ScheduledRunTransition::Succeed => {
                self.status = ScheduledRunStatus::Succeeded;
                self.failure_reason = None;
            }
            ScheduledRunTransition::Fail { reason } => {
                self.status = ScheduledRunStatus::Failed;
                self.failure_reason = Some(reason);
            }
            ScheduledRunTransition::Cancel => {
                self.status = ScheduledRunStatus::Cancelled;
                self.failure_reason = None;
            }
        }
        self.updated_at = transitioned_at;
        true
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{
        ScheduledMissReason, ScheduledOccurrence, ScheduledOccurrenceTiming, ScheduledRunStatus,
        ScheduledRunTransition, ScheduledTaskId, SessionId,
    };

    #[test]
    fn classifies_the_fixed_live_lateness_boundary() {
        let occurrence = ScheduledOccurrence {
            task_id: ScheduledTaskId::new("task-1"),
            scheduled_at: 1_000_000,
        };

        assert_eq!(
            occurrence.timing_at(999_999),
            ScheduledOccurrenceTiming::NotDue
        );
        assert_eq!(
            occurrence.timing_at(1_600_000),
            ScheduledOccurrenceTiming::Due
        );
        assert_eq!(
            occurrence.timing_at(1_600_001),
            ScheduledOccurrenceTiming::Missed(ScheduledMissReason::LateTrigger)
        );
    }

    #[test]
    fn only_accepted_and_running_states_occupy_admission_slots() {
        for status in [ScheduledRunStatus::Accepted, ScheduledRunStatus::Running] {
            assert!(status.is_active());
        }
        for status in [
            ScheduledRunStatus::Succeeded,
            ScheduledRunStatus::Failed,
            ScheduledRunStatus::Cancelled,
            ScheduledRunStatus::SkippedBusy,
            ScheduledRunStatus::SkippedCapacity,
        ] {
            assert!(!status.is_active());
        }
    }

    #[test]
    fn lifecycle_transitions_are_forward_only() {
        let start = ScheduledRunTransition::Start {
            session_id: SessionId::new("session-1"),
            worktree_path: "worktrees/run-1".to_string(),
        };
        assert!(start.is_allowed_from(ScheduledRunStatus::Accepted));
        assert!(!start.is_allowed_from(ScheduledRunStatus::Running));
        assert!(ScheduledRunTransition::Succeed.is_allowed_from(ScheduledRunStatus::Running));
        assert!(!ScheduledRunTransition::Succeed.is_allowed_from(ScheduledRunStatus::Accepted));
        assert!(!ScheduledRunTransition::Cancel.is_allowed_from(ScheduledRunStatus::Succeeded));
    }
}
