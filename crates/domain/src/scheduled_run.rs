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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{
        ScheduledMissReason, ScheduledOccurrence, ScheduledOccurrenceTiming, ScheduledRunStatus,
        ScheduledTaskId,
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
}
