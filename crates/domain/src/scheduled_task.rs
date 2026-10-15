use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{AgentRef, AuditFields, DomainModelError, ProjectId, ScheduledTaskId};

/// Names the only unattended approval policy supported by the first local release.
pub const FULL_AUTO_IN_WORKTREE_POLICY: &str = "full_auto_in_worktree";

/// Selects the local calendar days used by a weekly schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledTaskWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

/// Represents the bounded schedule choices exposed by the first release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduledTaskSchedule {
    EveryHours {
        interval: u16,
    },
    Daily {
        hour: u8,
        minute: u8,
    },
    Weekly {
        weekdays: BTreeSet<ScheduledTaskWeekday>,
        hour: u8,
        minute: u8,
    },
}

impl ScheduledTaskSchedule {
    /// Builds an hourly schedule while enforcing the first-release minimum interval.
    pub fn every_hours(interval: u16) -> Result<Self, DomainModelError> {
        if interval == 0 {
            return Err(DomainModelError::ScheduledTaskIntervalTooShort);
        }
        Ok(Self::EveryHours { interval })
    }

    /// Builds a daily schedule from a valid local wall-clock time.
    pub fn daily(hour: u8, minute: u8) -> Result<Self, DomainModelError> {
        validate_time(hour, minute)?;
        Ok(Self::Daily { hour, minute })
    }

    /// Builds a weekly schedule that selects at least one local weekday.
    pub fn weekly(
        weekdays: BTreeSet<ScheduledTaskWeekday>,
        hour: u8,
        minute: u8,
    ) -> Result<Self, DomainModelError> {
        if weekdays.is_empty() {
            return Err(DomainModelError::EmptyScheduledTaskWeekdays);
        }
        validate_time(hour, minute)?;
        Ok(Self::Weekly {
            weekdays,
            hour,
            minute,
        })
    }
}

/// Freezes where future occurrences are owned for the lifetime of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledTaskExecutionLocation {
    Local,
    Cloud,
}

/// Records the user's explicit consent for unattended local tool execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTaskApproval {
    pub policy: String,
    pub consented_at: i64,
}

impl ScheduledTaskApproval {
    /// Records consent for the first-release full-auto worktree policy.
    pub fn full_auto_in_worktree(consented_at: i64) -> Self {
        Self {
            policy: FULL_AUTO_IN_WORKTREE_POLICY.to_string(),
            consented_at,
        }
    }

    /// Reports whether this consent still names the supported first-release policy.
    pub fn is_supported(&self) -> bool {
        self.policy == FULL_AUTO_IN_WORKTREE_POLICY
    }
}

/// Stores one revisioned scheduled Prompt definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: ScheduledTaskId,
    pub name: String,
    pub enabled: bool,
    pub schedule: ScheduledTaskSchedule,
    pub execution_location: ScheduledTaskExecutionLocation,
    pub project_id: ProjectId,
    pub agent_ref: AgentRef,
    pub model: Option<String>,
    pub prompt: String,
    pub revision: u64,
    pub approval: Option<ScheduledTaskApproval>,
    pub audit_fields: AuditFields,
}

impl ScheduledTask {
    /// Creates a valid first revision without silently granting unattended execution.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ScheduledTaskId,
        name: impl Into<String>,
        enabled: bool,
        schedule: ScheduledTaskSchedule,
        execution_location: ScheduledTaskExecutionLocation,
        project_id: ProjectId,
        agent_ref: AgentRef,
        model: Option<String>,
        prompt: impl Into<String>,
        approval: Option<ScheduledTaskApproval>,
        audit_fields: AuditFields,
    ) -> Result<Self, DomainModelError> {
        let name = name.into().trim().to_string();
        if name.is_empty() {
            return Err(DomainModelError::EmptyScheduledTaskName);
        }
        let prompt = prompt.into().trim().to_string();
        if prompt.is_empty() {
            return Err(DomainModelError::EmptyScheduledTaskPrompt);
        }
        Ok(Self {
            id,
            name,
            enabled,
            schedule,
            execution_location,
            project_id,
            agent_ref,
            model,
            prompt,
            revision: 1,
            approval,
            audit_fields,
        })
    }

    /// Determines whether the task may be activated by the local scheduler.
    pub fn local_activation_allowed(&self) -> bool {
        self.execution_location == ScheduledTaskExecutionLocation::Local
            && self
                .approval
                .as_ref()
                .is_some_and(ScheduledTaskApproval::is_supported)
    }
}

fn validate_time(hour: u8, minute: u8) -> Result<(), DomainModelError> {
    if hour > 23 || minute > 59 {
        return Err(DomainModelError::InvalidScheduledTaskTime);
    }
    Ok(())
}
