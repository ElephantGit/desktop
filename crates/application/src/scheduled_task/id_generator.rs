use ora_domain::ScheduledTaskId;
use uuid::Uuid;

use super::ScheduledTaskIdGenerator;

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
