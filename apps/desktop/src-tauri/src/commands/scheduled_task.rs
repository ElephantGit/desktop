//! Desktop Scheduled Task definition operations.

use std::path::PathBuf;

use ora_backend::{BackendError, RequestLifecycle, UuidRequestIdGenerator};
use ora_contracts::*;
use serde::Deserialize;
use tauri::State;

use crate::error::CommandError;
use crate::state::DesktopState;

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
backend_command!(
    list_scheduled_run_artifacts,
    ListScheduledRunArtifactsRequest,
    ListScheduledRunArtifactsResponse,
    scheduled_tasks.list_run_artifacts,
    "Lists one Scheduled Run's collected artifacts and rejected candidates."
);
backend_command!(
    read_scheduled_run_artifact,
    ReadScheduledRunArtifactRequest,
    ReadScheduledRunArtifactResponse,
    scheduled_tasks.read_run_artifact,
    "Returns a bounded text preview of one Scheduled Run artifact."
);

/// Carries one registered artifact and the native save dialog's destination.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadScheduledRunArtifactRequest {
    run_id: String,
    relative_path: String,
    destination: PathBuf,
}

/// Copies one Scheduled Run artifact to a user-selected destination.
#[tauri::command]
pub async fn download_scheduled_run_artifact(
    state: State<'_, DesktopState>,
    request: DownloadScheduledRunArtifactRequest,
) -> Result<(), CommandError> {
    let lifecycle =
        RequestLifecycle::start("download_scheduled_run_artifact", &UuidRequestIdGenerator);
    let backend = state.backend.scheduled_tasks();
    let result = tauri::async_runtime::spawn_blocking(move || {
        backend.copy_run_artifact(
            &request.run_id,
            &request.relative_path,
            &request.destination,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => {
            lifecycle.complete_success();
            Ok(())
        }
        Ok(Err(error)) => Err(CommandError::from_backend_with_lifecycle(error, &lifecycle)),
        Err(source) => Err(CommandError::from_backend_with_lifecycle(
            BackendError::internal("scheduled run artifact export failed", source),
            &lifecycle,
        )),
    }
}
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
