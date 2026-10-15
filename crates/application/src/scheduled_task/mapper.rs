use ora_contracts::{
    ScheduledTask as ContractScheduledTask, ScheduledTaskApproval as ContractApproval,
    ScheduledTaskApprovalPolicy as ContractApprovalPolicy,
    ScheduledTaskExecutionLocation as ContractExecutionLocation,
    ScheduledTaskSchedule as ContractSchedule, ScheduledTaskWeekday as ContractWeekday,
};
use ora_domain::{
    ScheduledTask, ScheduledTaskApproval, ScheduledTaskExecutionLocation, ScheduledTaskSchedule,
    ScheduledTaskWeekday,
};

use crate::ApplicationError;

/// Validates and converts the public schedule shape into the domain representation.
pub(crate) fn parse_schedule(
    schedule: ContractSchedule,
) -> Result<ScheduledTaskSchedule, ApplicationError> {
    match schedule {
        ContractSchedule::EveryHours { interval } => ScheduledTaskSchedule::every_hours(interval),
        ContractSchedule::Daily { hour, minute } => ScheduledTaskSchedule::daily(hour, minute),
        ContractSchedule::Weekly {
            weekdays,
            hour,
            minute,
        } => ScheduledTaskSchedule::weekly(
            weekdays.into_iter().map(map_weekday_to_domain).collect(),
            hour,
            minute,
        ),
    }
    .map_err(ApplicationError::from_scheduled_task_domain_error)
}

pub(crate) fn approval_from_policy(
    policy: ContractApprovalPolicy,
    consented_at: i64,
) -> ScheduledTaskApproval {
    match policy {
        ContractApprovalPolicy::FullAutoInWorktree => {
            ScheduledTaskApproval::full_auto_in_worktree(consented_at)
        }
    }
}

/// Projects one domain task into the public contract form.
pub(crate) fn map_task(task: ScheduledTask) -> ContractScheduledTask {
    ContractScheduledTask {
        id: task.id.to_string(),
        name: task.name,
        enabled: task.enabled,
        schedule: map_schedule_to_contract(task.schedule),
        execution_location: match task.execution_location {
            ScheduledTaskExecutionLocation::Local => ContractExecutionLocation::Local,
            ScheduledTaskExecutionLocation::Cloud => ContractExecutionLocation::Cloud,
        },
        project_id: task.project_id.to_string(),
        agent_ref: task.agent_ref.to_string(),
        model: task.model,
        prompt: task.prompt,
        revision: task.revision,
        approval: task.approval.and_then(map_approval),
        created_at: task.audit_fields.created_at,
        updated_at: task.audit_fields.updated_at,
    }
}

fn map_approval(approval: ScheduledTaskApproval) -> Option<ContractApproval> {
    approval.is_supported().then_some(ContractApproval {
        policy: ContractApprovalPolicy::FullAutoInWorktree,
        consented_at: approval.consented_at,
    })
}

fn map_schedule_to_contract(schedule: ScheduledTaskSchedule) -> ContractSchedule {
    match schedule {
        ScheduledTaskSchedule::EveryHours { interval } => ContractSchedule::EveryHours { interval },
        ScheduledTaskSchedule::Daily { hour, minute } => ContractSchedule::Daily { hour, minute },
        ScheduledTaskSchedule::Weekly {
            weekdays,
            hour,
            minute,
        } => ContractSchedule::Weekly {
            weekdays: weekdays.into_iter().map(map_weekday_to_contract).collect(),
            hour,
            minute,
        },
    }
}

fn map_weekday_to_domain(weekday: ContractWeekday) -> ScheduledTaskWeekday {
    match weekday {
        ContractWeekday::Monday => ScheduledTaskWeekday::Monday,
        ContractWeekday::Tuesday => ScheduledTaskWeekday::Tuesday,
        ContractWeekday::Wednesday => ScheduledTaskWeekday::Wednesday,
        ContractWeekday::Thursday => ScheduledTaskWeekday::Thursday,
        ContractWeekday::Friday => ScheduledTaskWeekday::Friday,
        ContractWeekday::Saturday => ScheduledTaskWeekday::Saturday,
        ContractWeekday::Sunday => ScheduledTaskWeekday::Sunday,
    }
}

fn map_weekday_to_contract(weekday: ScheduledTaskWeekday) -> ContractWeekday {
    match weekday {
        ScheduledTaskWeekday::Monday => ContractWeekday::Monday,
        ScheduledTaskWeekday::Tuesday => ContractWeekday::Tuesday,
        ScheduledTaskWeekday::Wednesday => ContractWeekday::Wednesday,
        ScheduledTaskWeekday::Thursday => ContractWeekday::Thursday,
        ScheduledTaskWeekday::Friday => ContractWeekday::Friday,
        ScheduledTaskWeekday::Saturday => ContractWeekday::Saturday,
        ScheduledTaskWeekday::Sunday => ContractWeekday::Sunday,
    }
}
