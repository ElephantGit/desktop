use serde::{Deserialize, Serialize};

use crate::{
    AgentRef, ProjectId, ScheduledRunId, ScheduledTask, ScheduledTaskApproval, ScheduledTaskId,
    SessionId, TaskId,
};

/// Allows a live scheduler observation to arrive no more than ten minutes late.
pub const MAX_SCHEDULED_OCCURRENCE_LATENESS_MILLIS: i64 = 10 * 60 * 1_000;

/// Caps simultaneous local Scheduled Runs in the first release.
pub const MAX_CONCURRENT_LOCAL_SCHEDULED_RUNS: i64 = 4;

/// Bounds persisted run and missed history for one Scheduled Task.
pub const MAX_SCHEDULED_TASK_HISTORY_ENTRIES: i64 = 100;

/// Identifies one planned trigger independently from the task revision that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledOccurrence {
    pub task_id: ScheduledTaskId,
    pub scheduled_at: i64,
}

impl ScheduledOccurrence {
    /// Classifies a live scheduler observation without creating a persisted fact early.
    pub fn timing_at(&self, observed_at: i64) -> ScheduledOccurrenceTiming {
        if observed_at < self.scheduled_at {
            return ScheduledOccurrenceTiming::NotDue;
        }
        if observed_at.saturating_sub(self.scheduled_at) <= MAX_SCHEDULED_OCCURRENCE_LATENESS_MILLIS
        {
            return ScheduledOccurrenceTiming::Due;
        }
        ScheduledOccurrenceTiming::Missed(ScheduledMissReason::LateTrigger)
    }
}

/// Describes whether a live scheduled time may be admitted at the observation instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledOccurrenceTiming {
    NotDue,
    Due,
    Missed(ScheduledMissReason),
}

/// Records why a planned occurrence was observed without creating a Scheduled Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledMissReason {
    AppNotRunning,
    LateTrigger,
}

/// Stores a task-level missed fact rather than fabricating a failed Scheduled Run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledMissedOccurrence {
    pub occurrence: ScheduledOccurrence,
    pub reason: ScheduledMissReason,
    pub observed_at: i64,
}

/// Presents one persisted run or missed fact in task history order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduledTaskHistoryEntry {
    Run(Box<ScheduledRun>),
    Missed(ScheduledMissedOccurrence),
}

/// Freezes the execution inputs selected when an occurrence is first admitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledRunSnapshot {
    pub revision: u64,
    pub project_id: ProjectId,
    /// Preserves the unattended worktree baseline selected at occurrence admission.
    pub worktree_base_branch: Option<String>,
    pub agent_ref: AgentRef,
    pub model: Option<String>,
    pub prompt: String,
    pub approval: Option<ScheduledTaskApproval>,
}

/// Represents every lifecycle state defined for a persisted Scheduled Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledRunStatus {
    Accepted,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    SkippedBusy,
    SkippedCapacity,
}

impl ScheduledRunStatus {
    /// Reports whether this state occupies the task and global admission slots.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Accepted | Self::Running)
    }
}

/// Records the stable reason a Scheduled Run could not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledRunFailureReason {
    TargetUnavailable,
    ModelUnavailable,
    WorktreeFailed,
    ApprovalPolicyInvalid,
    Timeout,
    ExecutionError,
}

/// Independent, machine-readable business conclusion a run's agent reports in
/// addition to the execution-layer status. It never derives from [`ScheduledRunStatus`]
/// or [`ScheduledRunFailureReason`]: a run can be `Succeeded` while its business
/// verdict is `Fail`, and vice versa for infrastructure-only outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledRunVerdict {
    Pass,
    Warn,
    Fail,
    Blocked,
    Idle,
    RangeUnresolved,
}

impl ScheduledRunVerdict {
    /// Returns the stable column and contract representation of this verdict.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
            Self::Blocked => "blocked",
            Self::Idle => "idle",
            Self::RangeUnresolved => "range_unresolved",
        }
    }

    /// Parses the case-insensitive contract token back into a verdict.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pass" => Some(Self::Pass),
            "warn" => Some(Self::Warn),
            "fail" => Some(Self::Fail),
            "blocked" => Some(Self::Blocked),
            "idle" => Some(Self::Idle),
            "range_unresolved" => Some(Self::RangeUnresolved),
            _ => None,
        }
    }
}

/// Bounds the free-text detail an agent may attach to a verdict, in Unicode characters.
pub const MAX_SCHEDULED_RUN_VERDICT_DETAIL_CHARS: usize = 2_000;

/// One parsed business verdict and the free-text detail reported alongside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledRunVerdictAnnotation {
    pub verdict: ScheduledRunVerdict,
    pub detail: Option<String>,
}

/// Parses the documented verdict sentinel out of the final Assistant turn text.
///
/// The frozen contract (approved ADR) matches a line of the form
/// `QA_VERDICT: <pass|warn|fail|blocked|idle|range_unresolved>` followed optionally
/// by the immediately next line `QA_DETAIL: <free text>`. Marker and value are
/// case-insensitive, end-of-line whitespace is ignored, a match never crosses a line
/// boundary, and the first marker occurrence wins: a first marker carrying an illegal
/// value is a format violation and yields "not reported" rather than scanning further.
/// A detail longer than [`MAX_SCHEDULED_RUN_VERDICT_DETAIL_CHARS`] is likewise rejected.
///
/// Returns `None` when no marker is present, the value is illegal, or the detail is
/// over-long; callers treat `None` as "not reported" and never rewrite run facts.
pub fn parse_scheduled_run_verdict(text: &str) -> Option<ScheduledRunVerdictAnnotation> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.split('\n');
    while let Some(line) = lines.next() {
        let lower = line.to_ascii_lowercase();
        let Some(marker_start) = lower.find("qa_verdict:") else {
            continue;
        };
        let after = &line[marker_start + "qa_verdict:".len()..];
        let verdict = ScheduledRunVerdict::parse(after)?;
        let detail = lines.next().and_then(|next| {
            let next_lower = next.to_ascii_lowercase();
            let detail_start = next_lower.find("qa_detail:")?;
            Some(next[detail_start + "qa_detail:".len()..].trim().to_string())
        });
        if detail
            .as_ref()
            .is_some_and(|value| value.chars().count() > MAX_SCHEDULED_RUN_VERDICT_DETAIL_CHARS)
        {
            return None;
        }
        return Some(ScheduledRunVerdictAnnotation { verdict, detail });
    }
    None
}

/// Names one forward-only lifecycle change requested by the local execution adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledRunTransition {
    Start {
        session_id: SessionId,
        worktree_path: String,
        /// Links the run to the task-owned worktree created for this execution so history
        /// can open or discard the retained worktree after the process restarts.
        worktree_task_id: TaskId,
    },
    Succeed,
    Fail {
        reason: ScheduledRunFailureReason,
    },
    Cancel,
}

impl ScheduledRunTransition {
    /// Rejects duplicate, backwards, and terminal-state transitions.
    pub fn is_allowed_from(&self, status: ScheduledRunStatus) -> bool {
        match self {
            Self::Start { .. } => status == ScheduledRunStatus::Accepted,
            Self::Succeed => status == ScheduledRunStatus::Running,
            Self::Fail { .. } | Self::Cancel => status.is_active(),
        }
    }
}

/// Stores one admitted occurrence and the immutable inputs it will execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledRun {
    pub id: ScheduledRunId,
    pub occurrence: ScheduledOccurrence,
    pub snapshot: ScheduledRunSnapshot,
    pub status: ScheduledRunStatus,
    pub session_id: Option<SessionId>,
    pub worktree_path: Option<String>,
    /// The task-owned worktree created for this run, kept for retained-worktree history.
    pub worktree_task_id: Option<TaskId>,
    pub failure_reason: Option<ScheduledRunFailureReason>,
    /// Independent business conclusion reported by the run's agent; `None` until reported,
    /// and displayed as "not reported" without affecting the execution-layer status.
    pub verdict: Option<ScheduledRunVerdict>,
    /// Free-text detail the agent attaches to the verdict, bounded by the application layer.
    pub verdict_detail: Option<String>,
    /// When the verdict was annotated, independent of `updated_at` so retention ordering
    /// (which sorts by `updated_at`) is never affected by a verdict write.
    pub verdict_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ScheduledRun {
    /// Admits a planned occurrence by copying execution inputs out of the current definition.
    pub fn accepted(
        id: ScheduledRunId,
        occurrence: ScheduledOccurrence,
        task: &ScheduledTask,
        accepted_at: i64,
    ) -> Self {
        Self {
            id,
            occurrence,
            snapshot: ScheduledRunSnapshot {
                revision: task.revision,
                project_id: task.project_id.clone(),
                worktree_base_branch: task.worktree_base_branch.clone(),
                agent_ref: task.agent_ref.clone(),
                model: task.model.clone(),
                prompt: task.prompt.clone(),
                approval: task.approval.clone(),
            },
            status: ScheduledRunStatus::Accepted,
            session_id: None,
            worktree_path: None,
            worktree_task_id: None,
            failure_reason: None,
            verdict: None,
            verdict_detail: None,
            verdict_at: None,
            created_at: accepted_at,
            updated_at: accepted_at,
        }
    }

    /// Applies one valid forward transition while preserving execution references for history.
    pub fn transition(&mut self, transition: ScheduledRunTransition, transitioned_at: i64) -> bool {
        if !transition.is_allowed_from(self.status) {
            return false;
        }

        match transition {
            ScheduledRunTransition::Start {
                session_id,
                worktree_path,
                worktree_task_id,
            } => {
                // The ownership commit binds the run to its provisioned worktree
                // before the session starts, so Start must reuse that binding: a
                // mismatching one would rebind history to a different worktree
                // instead of starting the already-owned one.
                if self
                    .worktree_task_id
                    .as_ref()
                    .is_some_and(|existing| *existing != worktree_task_id)
                    || self
                        .worktree_path
                        .as_deref()
                        .is_some_and(|existing| existing != worktree_path)
                {
                    return false;
                }
                self.status = ScheduledRunStatus::Running;
                self.session_id = Some(session_id);
                self.worktree_path = Some(worktree_path);
                self.worktree_task_id = Some(worktree_task_id);
                self.failure_reason = None;
            }
            ScheduledRunTransition::Succeed => {
                self.status = ScheduledRunStatus::Succeeded;
                self.failure_reason = None;
            }
            ScheduledRunTransition::Fail { reason } => {
                self.status = ScheduledRunStatus::Failed;
                self.failure_reason = Some(reason);
            }
            ScheduledRunTransition::Cancel => {
                self.status = ScheduledRunStatus::Cancelled;
                self.failure_reason = None;
            }
        }
        self.updated_at = transitioned_at;
        true
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{
        AgentRef, AuditFields, ProjectId, ScheduledMissReason, ScheduledOccurrence,
        ScheduledOccurrenceTiming, ScheduledRun, ScheduledRunId, ScheduledRunStatus,
        ScheduledRunTransition, ScheduledRunVerdict, ScheduledTask, ScheduledTaskApproval,
        ScheduledTaskExecutionLocation, ScheduledTaskId, ScheduledTaskSchedule, SessionId, TaskId,
    };

    #[test]
    fn classifies_the_fixed_live_lateness_boundary() {
        let occurrence = ScheduledOccurrence {
            task_id: ScheduledTaskId::new("task-1"),
            scheduled_at: 1_000_000,
        };

        assert_eq!(
            occurrence.timing_at(999_999),
            ScheduledOccurrenceTiming::NotDue
        );
        assert_eq!(
            occurrence.timing_at(1_600_000),
            ScheduledOccurrenceTiming::Due
        );
        assert_eq!(
            occurrence.timing_at(1_600_001),
            ScheduledOccurrenceTiming::Missed(ScheduledMissReason::LateTrigger)
        );
    }

    #[test]
    fn only_accepted_and_running_states_occupy_admission_slots() {
        for status in [ScheduledRunStatus::Accepted, ScheduledRunStatus::Running] {
            assert!(status.is_active());
        }
        for status in [
            ScheduledRunStatus::Succeeded,
            ScheduledRunStatus::Failed,
            ScheduledRunStatus::Cancelled,
            ScheduledRunStatus::SkippedBusy,
            ScheduledRunStatus::SkippedCapacity,
        ] {
            assert!(!status.is_active());
        }
    }

    #[test]
    fn lifecycle_transitions_are_forward_only() {
        let start = ScheduledRunTransition::Start {
            session_id: SessionId::new("session-1"),
            worktree_path: "worktrees/run-1".to_string(),
            worktree_task_id: TaskId::new("task-1"),
        };
        assert!(start.is_allowed_from(ScheduledRunStatus::Accepted));
        assert!(!start.is_allowed_from(ScheduledRunStatus::Running));
        assert!(ScheduledRunTransition::Succeed.is_allowed_from(ScheduledRunStatus::Running));
        assert!(!ScheduledRunTransition::Succeed.is_allowed_from(ScheduledRunStatus::Accepted));
        assert!(!ScheduledRunTransition::Cancel.is_allowed_from(ScheduledRunStatus::Succeeded));
    }

    #[test]
    fn admitted_run_freezes_execution_inputs_at_its_task_revision() {
        let approval = ScheduledTaskApproval::full_auto_in_worktree(10);
        let mut task = ScheduledTask::new(
            ScheduledTaskId::new("task-1"),
            "Code quality",
            true,
            ScheduledTaskSchedule::daily(9, 30).expect("valid schedule"),
            ScheduledTaskExecutionLocation::Local,
            ProjectId::new("project-1"),
            AgentRef::parse("ora-space/codex").expect("valid agent"),
            Some("model-1".to_string()),
            "Run the current checks",
            Some(approval.clone()),
            AuditFields::new(1, 1, false),
        )
        .expect("valid task");
        task.revision = 3;

        let run = ScheduledRun::accepted(
            ScheduledRunId::new("run-1"),
            ScheduledOccurrence {
                task_id: task.id.clone(),
                scheduled_at: 1_000,
            },
            &task,
            1_001,
        );

        task.revision = 4;
        task.project_id = ProjectId::new("project-2");
        task.model = Some("model-2".to_string());
        task.prompt = "Run a different task".to_string();
        task.approval = None;

        assert_eq!(run.snapshot.revision, 3);
        assert_eq!(run.snapshot.project_id, ProjectId::new("project-1"));
        assert_eq!(
            run.snapshot.agent_ref,
            AgentRef::parse("ora-space/codex").unwrap()
        );
        assert_eq!(run.snapshot.model.as_deref(), Some("model-1"));
    }

    #[test]
    fn start_reuses_the_committed_worktree_binding_and_rejects_a_mismatch() {
        let owned_path = "worktrees/owned".to_string();
        let owned_task = TaskId::new("task-owned");
        let mut run = ScheduledRun::accepted(
            ScheduledRunId::new("run-1"),
            ScheduledOccurrence {
                task_id: ScheduledTaskId::new("task-1"),
                scheduled_at: 1_000,
            },
            &scheduled_task(),
            1_001,
        );
        run.worktree_path = Some(owned_path.clone());
        run.worktree_task_id = Some(owned_task.clone());

        // A matching Start keeps the committed binding; the run becomes running
        // without rebinding history to a different worktree.
        assert!(run.transition(
            ScheduledRunTransition::Start {
                session_id: SessionId::new("session-1"),
                worktree_path: owned_path.clone(),
                worktree_task_id: owned_task.clone(),
            },
            1_100,
        ));
        assert_eq!(run.status, ScheduledRunStatus::Running);
        assert_eq!(run.worktree_path.as_deref(), Some(owned_path.as_str()));
        assert_eq!(run.worktree_task_id, Some(owned_task.clone()));

        // A mismatching rebinding is rejected and leaves the run untouched.
        let mut rebinding = ScheduledRun::accepted(
            ScheduledRunId::new("run-2"),
            ScheduledOccurrence {
                task_id: ScheduledTaskId::new("task-1"),
                scheduled_at: 2_000,
            },
            &scheduled_task(),
            2_001,
        );
        rebinding.worktree_path = Some("worktrees/other".to_string());
        rebinding.worktree_task_id = Some(TaskId::new("task-other"));
        assert!(!rebinding.transition(
            ScheduledRunTransition::Start {
                session_id: SessionId::new("session-2"),
                worktree_path: "worktrees/other".to_string(),
                worktree_task_id: TaskId::new("task-unrelated"),
            },
            2_100,
        ));
        assert_eq!(rebinding.status, ScheduledRunStatus::Accepted);
        assert_eq!(rebinding.worktree_task_id, Some(TaskId::new("task-other")));
    }

    /// Builds one valid definition for lifecycle tests.
    fn scheduled_task() -> ScheduledTask {
        ScheduledTask::new(
            ScheduledTaskId::new("task-1"),
            "Code quality",
            true,
            ScheduledTaskSchedule::daily(9, 30).expect("valid schedule"),
            ScheduledTaskExecutionLocation::Local,
            ProjectId::new("project-1"),
            AgentRef::parse("ora-space/codex").expect("valid agent"),
            None,
            "Run the current checks",
            Some(ScheduledTaskApproval::full_auto_in_worktree(10)),
            AuditFields::new(1, 1, false),
        )
        .expect("valid task")
    }

    /// Verifies the stable column/contract representation and the default absence of a
    /// verdict on a freshly admitted run.
    #[test]
    fn verdict_has_a_stable_representation_and_defaults_to_unreported() {
        use crate::ScheduledRunVerdict;

        assert_eq!(ScheduledRunVerdict::Pass.as_str(), "pass");
        assert_eq!(ScheduledRunVerdict::Warn.as_str(), "warn");
        assert_eq!(ScheduledRunVerdict::Fail.as_str(), "fail");
        assert_eq!(ScheduledRunVerdict::Blocked.as_str(), "blocked");
        assert_eq!(ScheduledRunVerdict::Idle.as_str(), "idle");
        assert_eq!(
            ScheduledRunVerdict::RangeUnresolved.as_str(),
            "range_unresolved"
        );

        let run = ScheduledRun::accepted(
            ScheduledRunId::new("run-1"),
            ScheduledOccurrence {
                task_id: ScheduledTaskId::new("task-1"),
                scheduled_at: 1_000,
            },
            &scheduled_task(),
            1_001,
        );
        assert_eq!(run.verdict, None);
        assert_eq!(run.verdict_detail, None);
    }

    fn parse(text: &str) -> Option<(crate::ScheduledRunVerdict, Option<String>)> {
        crate::parse_scheduled_run_verdict(text)
            .map(|annotation| (annotation.verdict, annotation.detail))
    }

    #[test]
    fn parses_the_case_insensitive_sentinel_and_following_detail() {
        assert_eq!(
            parse("QA_VERDICT: pass"),
            Some((ScheduledRunVerdict::Pass, None))
        );
        assert_eq!(
            parse("qa_verdict: Fail\r\nQA_DETAIL: Critical overflow\r\n"),
            Some((
                ScheduledRunVerdict::Fail,
                Some("Critical overflow".to_string())
            ))
        );
        assert_eq!(
            parse("inspect\ndone\nQA_VERDICT: range_unresolved "),
            Some((ScheduledRunVerdict::RangeUnresolved, None))
        );
    }

    #[test]
    fn takes_the_first_marker_and_rejects_an_illegal_value() {
        assert_eq!(
            parse("QA_VERDICT: pass\nQA_VERDICT: fail"),
            Some((ScheduledRunVerdict::Pass, None))
        );
        assert_eq!(parse("QA_VERDICT: banana\nQA_VERDICT: pass"), None);
        assert_eq!(parse("no marker here"), None);
    }

    #[test]
    fn only_reads_detail_from_the_immediately_following_line() {
        assert_eq!(
            parse("QA_VERDICT: warn\nsome other line\nQA_DETAIL: late"),
            Some((ScheduledRunVerdict::Warn, None))
        );
    }

    #[test]
    fn rejects_an_over_long_detail() {
        let long = "x".repeat(crate::MAX_SCHEDULED_RUN_VERDICT_DETAIL_CHARS + 1);
        let text = format!("QA_VERDICT: fail\nQA_DETAIL: {long}");
        assert_eq!(parse(&text), None);

        let boundary = "y".repeat(crate::MAX_SCHEDULED_RUN_VERDICT_DETAIL_CHARS);
        let text = format!("QA_VERDICT: fail\nQA_DETAIL: {boundary}");
        assert_eq!(
            parse(&text),
            Some((ScheduledRunVerdict::Fail, Some(boundary)))
        );
    }

    #[test]
    fn ignores_non_verdict_marker_prefixes_without_a_valid_token() {
        assert_eq!(
            parse("results: QA_VERDICT: pass"),
            Some((ScheduledRunVerdict::Pass, None))
        );
    }
}
