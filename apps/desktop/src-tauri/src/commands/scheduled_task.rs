//! Desktop Scheduled Task definition operations.

use ora_contracts::*;

async_backend_command!(
    create_scheduled_task,
    CreateScheduledTaskRequest,
    CreateScheduledTaskResponse,
    scheduled_tasks.create,
    "Creates one Scheduled Task definition through the shared Backend."
);
backend_command!(
    get_scheduled_task,
    GetScheduledTaskRequest,
    GetScheduledTaskResponse,
    scheduled_tasks.get,
    "Gets one Scheduled Task definition through the shared Backend."
);
backend_command!(
    list_scheduled_tasks,
    ListScheduledTasksRequest,
    ListScheduledTasksResponse,
    scheduled_tasks.list,
    "Lists Scheduled Task definitions through the shared Backend."
);
backend_command!(
    list_scheduled_task_history,
    ListScheduledTaskHistoryRequest,
    ListScheduledTaskHistoryResponse,
    scheduled_tasks.list_history,
    "Lists persisted Scheduled Task run and missed history through the shared Backend."
);
async_backend_command!(
    update_scheduled_task,
    UpdateScheduledTaskRequest,
    UpdateScheduledTaskResponse,
    scheduled_tasks.update,
    "Updates one Scheduled Task definition through the shared Backend."
);
backend_command!(
    delete_scheduled_task,
    DeleteScheduledTaskRequest,
    DeleteScheduledTaskResponse,
    scheduled_tasks.delete,
    "Soft-deletes one Scheduled Task definition through the shared Backend."
);
backend_command!(
    revoke_scheduled_task_approval,
    RevokeScheduledTaskApprovalRequest,
    RevokeScheduledTaskApprovalResponse,
    scheduled_tasks.revoke_approval,
    "Revokes local-background approval for one Scheduled Task."
);
