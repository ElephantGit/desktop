use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::AgentRef;

/// Selects one local weekday for a weekly scheduled task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "scheduled-task.ts")]
pub enum ScheduledTaskWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

/// Describes one of the bounded first-release schedule shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export_to = "scheduled-task.ts")]
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

/// Identifies which runtime owns every future occurrence of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "scheduled-task.ts")]
pub enum ScheduledTaskExecutionLocation {
    Local,
    Cloud,
}

/// Names an unattended execution policy the user has explicitly accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "scheduled-task.ts")]
pub enum ScheduledTaskApprovalPolicy {
    FullAutoInWorktree,
}

/// Returns the persisted consent fact without exposing runtime implementation details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct ScheduledTaskApproval {
    pub policy: ScheduledTaskApprovalPolicy,
    pub consented_at: i64,
}

/// Public scheduled task definition shown in the Desktop list and editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: ScheduledTaskSchedule,
    pub execution_location: ScheduledTaskExecutionLocation,
    pub project_id: String,
    pub agent_ref: AgentRef,
    pub model: Option<String>,
    pub prompt: String,
    pub revision: u64,
    pub approval: Option<ScheduledTaskApproval>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Carries the editable fields used to create one local scheduled task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct CreateScheduledTaskRequest {
    pub name: String,
    pub enabled: bool,
    pub schedule: ScheduledTaskSchedule,
    pub execution_location: ScheduledTaskExecutionLocation,
    pub project_id: String,
    pub agent_ref: AgentRef,
    pub model: Option<String>,
    pub prompt: String,
    pub approval_policy: Option<ScheduledTaskApprovalPolicy>,
}

/// Returns the newly persisted scheduled task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct CreateScheduledTaskResponse {
    pub task: ScheduledTask,
}

/// Identifies one scheduled task definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct GetScheduledTaskRequest {
    pub task_id: String,
}

/// Returns one scheduled task definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct GetScheduledTaskResponse {
    pub task: ScheduledTask,
}

/// Requests every visible scheduled task definition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct ListScheduledTasksRequest {}

/// Returns every visible scheduled task definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct ListScheduledTasksResponse {
    pub tasks: Vec<ScheduledTask>,
}

/// Replaces editable fields while preserving execution ownership and consent rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct UpdateScheduledTaskRequest {
    pub task_id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: ScheduledTaskSchedule,
    pub project_id: String,
    pub agent_ref: AgentRef,
    pub model: Option<String>,
    pub prompt: String,
    pub approval_policy: Option<ScheduledTaskApprovalPolicy>,
}

/// Returns the authoritative updated task snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct UpdateScheduledTaskResponse {
    pub task: ScheduledTask,
}

/// Identifies a task to soft-delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct DeleteScheduledTaskRequest {
    pub task_id: String,
}

/// Confirms which task definition was soft-deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct DeleteScheduledTaskResponse {
    pub task_id: String,
}

/// Revokes unattended consent and disables the task without rewriting history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct RevokeScheduledTaskApprovalRequest {
    pub task_id: String,
}

/// Returns the disabled task after consent is revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scheduled-task.ts")]
pub struct RevokeScheduledTaskApprovalResponse {
    pub task: ScheduledTask,
}

/// Exports every Scheduled Task TypeScript binding into the generated contracts package.
pub(crate) fn export(config: &ts_rs::Config) -> Result<(), ts_rs::ExportError> {
    ScheduledTaskWeekday::export(config)?;
    ScheduledTaskSchedule::export(config)?;
    ScheduledTaskExecutionLocation::export(config)?;
    ScheduledTaskApprovalPolicy::export(config)?;
    ScheduledTaskApproval::export(config)?;
    ScheduledTask::export(config)?;
    CreateScheduledTaskRequest::export(config)?;
    CreateScheduledTaskResponse::export(config)?;
    GetScheduledTaskRequest::export(config)?;
    GetScheduledTaskResponse::export(config)?;
    ListScheduledTasksRequest::export(config)?;
    ListScheduledTasksResponse::export(config)?;
    UpdateScheduledTaskRequest::export(config)?;
    UpdateScheduledTaskResponse::export(config)?;
    DeleteScheduledTaskRequest::export(config)?;
    DeleteScheduledTaskResponse::export(config)?;
    RevokeScheduledTaskApprovalRequest::export(config)?;
    RevokeScheduledTaskApprovalResponse::export(config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{ScheduledTaskSchedule, ScheduledTaskWeekday};

    #[test]
    fn weekly_schedule_uses_the_public_tagged_shape() {
        let schedule = ScheduledTaskSchedule::Weekly {
            weekdays: BTreeSet::from([
                ScheduledTaskWeekday::Monday,
                ScheduledTaskWeekday::Friday,
            ]),
            hour: 9,
            minute: 30,
        };

        assert_eq!(
            serde_json::to_value(schedule).expect("serialize weekly schedule"),
            json!({
                "kind": "weekly",
                "weekdays": ["monday", "friday"],
                "hour": 9,
                "minute": 30
            })
        );
    }
}
