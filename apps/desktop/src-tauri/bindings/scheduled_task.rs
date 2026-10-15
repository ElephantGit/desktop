//! Desktop bindings for Scheduled Task definitions.

use super::{Binding, Permission};

pub(super) const BINDINGS: &[Binding] = &[
    Binding::Unary {
        operation: "createScheduledTask",
        handler: "commands::scheduled_task::create_scheduled_task",
        permission: Permission::MainWebview,
    },
    Binding::Unary {
        operation: "getScheduledTask",
        handler: "commands::scheduled_task::get_scheduled_task",
        permission: Permission::MainWebview,
    },
    Binding::Unary {
        operation: "listScheduledTasks",
        handler: "commands::scheduled_task::list_scheduled_tasks",
        permission: Permission::MainWebview,
    },
    Binding::Unary {
        operation: "listScheduledTaskHistory",
        handler: "commands::scheduled_task::list_scheduled_task_history",
        permission: Permission::MainWebview,
    },
    Binding::Unary {
        operation: "updateScheduledTask",
        handler: "commands::scheduled_task::update_scheduled_task",
        permission: Permission::MainWebview,
    },
    Binding::Unary {
        operation: "deleteScheduledTask",
        handler: "commands::scheduled_task::delete_scheduled_task",
        permission: Permission::MainWebview,
    },
    Binding::Unary {
        operation: "revokeScheduledTaskApproval",
        handler: "commands::scheduled_task::revoke_scheduled_task_approval",
        permission: Permission::MainWebview,
    },
];
