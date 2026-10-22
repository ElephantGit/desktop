mod admission;
mod artifact_handlers;
mod artifact_store;
mod handlers;
mod id_generator;
mod mapper;
mod offline_recovery;
mod ports;
mod target_validator;

#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod offline_recovery_tests;
#[cfg(test)]
mod tests;

pub use admission::AdmitScheduledOccurrenceHandler;
pub use artifact_handlers::{
    CollectScheduledRunArtifactsHandler, CopyScheduledRunArtifactHandler,
    ListScheduledRunArtifactsHandler, ReadScheduledRunArtifactHandler,
    SweepScheduledRunArtifactsHandler,
};
pub use artifact_store::{
    CollectedScheduledArtifacts, ScheduledArtifactStore, ScheduledArtifactStoreError,
};
pub use handlers::{
    CreateScheduledTaskHandler, DeleteScheduledTaskHandler, GetScheduledTaskHandler,
    ListScheduledTaskHistoryHandler, ListScheduledTasksHandler, PreparedScheduledTaskUpdate,
    RevokeScheduledTaskApprovalHandler, UpdateScheduledTaskHandler,
};
pub use id_generator::{UuidScheduledRunIdGenerator, UuidScheduledTaskIdGenerator};
pub use offline_recovery::{OfflineRecoveryResolution, RecordOfflineMissedOccurrenceHandler};
pub use ports::{
    ScheduledArtifactRunRecord, ScheduledOccurrenceCandidate, ScheduledOccurrenceRepository,
    ScheduledOccurrenceResolution, ScheduledRunArtifactRepository, ScheduledRunIdGenerator,
    ScheduledRunLifecycleRepository, ScheduledRunTransitionResolution,
    ScheduledTaskHistoryRepository, ScheduledTaskIdGenerator, ScheduledTaskRepository,
    ScheduledTaskTargetValidator, ScheduledTaskUpdateResolution,
};
pub use target_validator::GitScheduledTaskTargetValidator;
