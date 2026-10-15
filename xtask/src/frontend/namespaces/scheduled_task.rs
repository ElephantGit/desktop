//! Endpoint declarations for the Scheduled Task generated-client namespace.

use crate::frontend::{FrontendEndpoint, FrontendResponseMode};

const NAMESPACE: &str = "scheduledTask";

pub(super) const ENDPOINTS: &[FrontendEndpoint] = &[
    FrontendEndpoint {
        operation_name: "createScheduledTask",
        namespace: NAMESPACE,
        member_name: "create",
        request_type: "CreateScheduledTaskRequest",
        response_type: "CreateScheduledTaskResponse",
        response_mode: FrontendResponseMode::Unary,
    },
    FrontendEndpoint {
        operation_name: "getScheduledTask",
        namespace: NAMESPACE,
        member_name: "get",
        request_type: "GetScheduledTaskRequest",
        response_type: "GetScheduledTaskResponse",
        response_mode: FrontendResponseMode::Unary,
    },
    FrontendEndpoint {
        operation_name: "listScheduledTasks",
        namespace: NAMESPACE,
        member_name: "list",
        request_type: "ListScheduledTasksRequest",
        response_type: "ListScheduledTasksResponse",
        response_mode: FrontendResponseMode::Unary,
    },
    FrontendEndpoint {
        operation_name: "updateScheduledTask",
        namespace: NAMESPACE,
        member_name: "update",
        request_type: "UpdateScheduledTaskRequest",
        response_type: "UpdateScheduledTaskResponse",
        response_mode: FrontendResponseMode::Unary,
    },
    FrontendEndpoint {
        operation_name: "deleteScheduledTask",
        namespace: NAMESPACE,
        member_name: "delete",
        request_type: "DeleteScheduledTaskRequest",
        response_type: "DeleteScheduledTaskResponse",
        response_mode: FrontendResponseMode::Unary,
    },
    FrontendEndpoint {
        operation_name: "revokeScheduledTaskApproval",
        namespace: NAMESPACE,
        member_name: "revokeApproval",
        request_type: "RevokeScheduledTaskApprovalRequest",
        response_type: "RevokeScheduledTaskApprovalResponse",
        response_mode: FrontendResponseMode::Unary,
    },
];
