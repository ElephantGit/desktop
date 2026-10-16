use ora_domain::{ScheduledRunId, ScheduledTaskId};
use uuid::Uuid;

use super::{ScheduledRunIdGenerator, ScheduledTaskIdGenerator};

/// Generates UUID-backed scheduled-task identifiers.
#[derive(Clone, Copy, Debug, Default)]
pub struct UuidScheduledTaskIdGenerator;

impl UuidScheduledTaskIdGenerator {
    /// Builds the UUID-backed identifier generator.
    pub fn new() -> Self {
        Self
    }
}

impl ScheduledTaskIdGenerator for UuidScheduledTaskIdGenerator {
    fn generate_scheduled_task_id(&self) -> ScheduledTaskId {
        ScheduledTaskId::new(Uuid::new_v4().to_string())
    }
}

/// Generates UUID-backed Scheduled Run identifiers.
#[derive(Clone, Copy, Debug, Default)]
pub struct UuidScheduledRunIdGenerator;

impl UuidScheduledRunIdGenerator {
    /// Builds the UUID-backed identifier generator.
    pub fn new() -> Self {
        Self
    }
}

impl ScheduledRunIdGenerator for UuidScheduledRunIdGenerator {
    fn generate_scheduled_run_id(&self) -> ScheduledRunId {
        ScheduledRunId::new(Uuid::new_v4().to_string())
    }
}
