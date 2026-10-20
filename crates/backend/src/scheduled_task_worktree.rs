use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitlancer::git::diff::{DiffRequest, DiffScope};
use gitlancer::git::status::StatusEntriesRequest;
use gitlancer::git::worktree::FindWorktreeRequest;
use gitlancer::{CliGitRunner, CommitId, Git, RepoRoot, Repository};
use ora_application::{
    ScheduledRunLifecycleRepository, TaskActivityRepository, TaskRepository, WorktreeRepository,
};
use ora_contracts::DeleteTaskRequest;
use ora_db::{
    RepositoryPool, SqliteScheduledRunRepository, SqliteTaskRepository, SqliteWorktreeRepository,
};
use ora_domain::{ScheduledRun, ScheduledRunId, TaskId};

use crate::task::{TaskApi, resolve_project_cwd};

/// Upper bound on Scheduled Run worktrees kept across every task, chosen per the approved
/// ADR's implementation-stage freedom (specs `decisions/desktop/core/scheduled-task` D4).
pub(super) const MAX_RETAINED_SCHEDULED_WORKTREES: usize = 10;

/// One finished run's worktree as the retention decision sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RetentionCandidate {
    pub run_id: ScheduledRunId,
    pub task_id: TaskId,
    /// Whether the task-owned worktree is still a visible task row.
    pub task_exists: bool,
    /// Whether a live session or workflow run still occupies the worktree. In-use worktrees
    /// are never cleaned automatically; the user closes them through the normal stop flows.
    pub in_use: bool,
    /// `Some(true)` when the worktree carries any change against its provisioned baseline,
    /// `Some(false)` when it is provably unmodified, and `None` when it cannot be classified
    /// and must therefore be kept.
    pub has_changes: Option<bool>,
}

/// Cleans up task-owned worktrees after Scheduled Runs finish.
///
/// Unmodified worktrees are deleted through the regular task cascade so Git cleanup removes
/// the checkout and branch exactly like an interactive discard. Worktrees with changes are
/// retained for the user, bounded by [`MAX_RETAINED_SCHEDULED_WORKTREES`] with oldest-finished
/// runs deleted first. Worktrees still in use are skipped entirely.
#[derive(Clone)]
pub(super) struct ScheduledWorktreeJanitor {
    tasks: Arc<TaskApi>,
    pool: RepositoryPool,
    relative_path_base: PathBuf,
    runs: SqliteScheduledRunRepository,
    task_rows: SqliteTaskRepository,
    worktrees: SqliteWorktreeRepository,
}

impl ScheduledWorktreeJanitor {
    pub(super) fn new(
        tasks: Arc<TaskApi>,
        pool: RepositoryPool,
        relative_path_base: PathBuf,
    ) -> Self {
        Self {
            tasks,
            relative_path_base,
            runs: SqliteScheduledRunRepository::new(pool.clone()),
            task_rows: SqliteTaskRepository::new(pool.clone()),
            worktrees: SqliteWorktreeRepository::new(pool.clone()),
            pool,
        }
    }

    /// Decides keep-versus-delete for one finished run, then keeps the retention bound.
    ///
    /// A run that never started still owns a freshly provisioned worktree; it cannot carry
    /// changes and is discarded directly. A worktree whose task is still in use is untouched,
    /// and a failed existence probe skips the run entirely so a transient database error
    /// never unlinks a worktree that still exists.
    pub(super) async fn run_finished(
        &self,
        run: &ScheduledRun,
        worktree_task_id: &TaskId,
        worktree_path: Option<&str>,
    ) {
        if self.task_exists(worktree_task_id) != Some(true) || self.task_in_use(worktree_task_id) {
            return;
        }
        let classification = match worktree_path {
            Some(path) => self.classify_worktree(run, worktree_task_id, path),
            None => Some(false),
        };
        match classification {
            Some(false) => self.discard_task(worktree_task_id, &run.id).await,
            _ => self.enforce_retention().await,
        }
    }

    /// Enforces the retained-worktree bound across every task, deleting the oldest kept
    /// worktrees first. Also discards worktrees that provably carry no changes, which
    /// covers runs a previous process finished without cleanup. In-use worktrees are
    /// excluded from both deletion and the retained count, so the cap governs the unused set.
    pub(super) async fn enforce_retention(&self) {
        let Ok(runs) = self.runs.list_scheduled_runs_with_retained_worktrees() else {
            return;
        };
        let mut candidates = Vec::with_capacity(runs.len());
        for run in runs {
            let Some(task_id) = run.worktree_task_id.clone() else {
                continue;
            };
            let Some(worktree_path) = run.worktree_path.clone() else {
                continue;
            };
            match self.task_exists(&task_id) {
                // A transient probe failure must keep the link for the next sweep instead of
                // unlinking a worktree that probably still exists.
                None => continue,
                Some(false) => {
                    // History actions may delete a retained worktree through the ordinary
                    // Task API. Treat the missing row as an already-completed discard so
                    // this run can be pruned again.
                    if let Err(error) = self.runs.clear_retained_worktree_link(&run.id) {
                        ora_logging::ora_warn!(
                            run_id = %run.id,
                            error = %error,
                            "scheduled run kept a stale worktree link after its task was deleted"
                        );
                    }
                    continue;
                }
                Some(true) => {}
            }
            let in_use = self.task_in_use(&task_id);
            let has_changes = self.classify_worktree(&run, &task_id, &worktree_path);
            candidates.push(RetentionCandidate {
                run_id: run.id.clone(),
                task_id,
                task_exists: true,
                in_use,
                has_changes,
            });
        }
        for (task_id, run_id) in retention_discards(&candidates, MAX_RETAINED_SCHEDULED_WORKTREES) {
            self.discard_task(&task_id, &run_id).await;
        }
    }

    /// Reports whether the task row is visible. `None` means the probe failed and callers
    /// must treat the worktree as still present; only `Some(false)` proves the row is gone.
    fn task_exists(&self, task_id: &TaskId) -> Option<bool> {
        self.task_rows
            .find_task(task_id)
            .map(|task| task.is_some())
            .ok()
    }

    /// Reports whether a live session or workflow run still occupies the task workspace.
    /// A failed probe counts as in use so a transient database error never deletes a
    /// worktree that might be occupied.
    fn task_in_use(&self, task_id: &TaskId) -> bool {
        self.task_rows
            .task_has_running_activity(task_id)
            .unwrap_or(true)
    }

    /// Reports whether one run's worktree differs from its provisioned baseline.
    ///
    /// Uncommitted entries and committed changes against the recorded base commit both count
    /// as changes. Missing worktree directories report no changes; anything gitlancer cannot
    /// resolve returns `None` so the caller keeps the worktree instead of losing results.
    fn classify_worktree(
        &self,
        run: &ScheduledRun,
        task_id: &TaskId,
        worktree_path: &str,
    ) -> Option<bool> {
        let task = match self.task_rows.find_task(task_id) {
            Ok(Some(task)) => task,
            _ => return None,
        };
        let worktree = self.worktrees.find_worktree(&task.workspace_id).ok()??;
        let baseline = worktree.baseline.commit_id().map(str::to_string);
        let repository_root = resolve_project_cwd(
            &self.pool,
            &run.snapshot.project_id,
            &self.relative_path_base,
        )
        .ok()?;
        git_worktree_has_changes(&repository_root, worktree_path, baseline)
    }

    /// Discards one run's retained worktree through the regular task cascade and clears the
    /// run's retained-worktree link once the task row is gone, so the retention sweep and
    /// history pruning stop tracking it. A failed discard keeps the link for later sweeps.
    async fn discard_task(&self, task_id: &TaskId, run_id: &ScheduledRunId) {
        match self
            .tasks
            .clone()
            .delete(DeleteTaskRequest {
                task_id: task_id.to_string(),
            })
            .await
        {
            Ok(_) => {
                if let Err(error) = self.runs.clear_retained_worktree_link(run_id) {
                    ora_logging::ora_warn!(
                        run_id = %run_id,
                        error = %error,
                        "scheduled run kept its worktree link after a successful discard"
                    );
                }
            }
            Err(error) => {
                ora_logging::ora_warn!(
                    run_id = %run_id,
                    task_id = %task_id,
                    error = %error,
                    "scheduled run worktree discard failed; the worktree is kept"
                );
            }
        }
    }
}

/// Reports whether the worktree checkout at `worktree_path` differs from its baseline.
///
/// A missing checkout reports no changes; an absent baseline or a Git failure reports
/// `None` so callers keep the worktree instead of deleting possible results.
fn git_worktree_has_changes(
    repository_root: &Path,
    worktree_path: &str,
    baseline_commit_id: Option<String>,
) -> Option<bool> {
    if !Path::new(worktree_path).exists() {
        return Some(false);
    }
    let git = Git::new(CliGitRunner);
    let repository = Repository::new(RepoRoot::new(repository_root));
    let worktree = git
        .find_worktree(FindWorktreeRequest {
            repository: &repository,
            candidate_path: Path::new(worktree_path),
        })
        .ok()?;
    let status = git
        .status_entries(StatusEntriesRequest {
            worktree: &worktree,
        })
        .ok()?;
    if !status.entries.is_empty() {
        return Some(true);
    }
    let base_commit_id = baseline_commit_id?;
    let diff = git
        .diff(DiffRequest {
            worktree: &worktree,
            base_commit_id: Some(&CommitId::new(base_commit_id)),
            scope: DiffScope::Committed,
        })
        .ok()?;
    Some(!diff.patch.is_empty())
}

/// Selects the worktrees to discard from finished runs, oldest first.
///
/// Unmodified worktrees always go; modified ones fill the retained bound and anything past it
/// is discarded oldest-first. Unclassifiable worktrees count as modified. Candidates whose
/// task row already disappeared have nothing to discard, and in-use worktrees are excluded
/// from both deletion and the retained count, so the cap governs the unused set.
pub(super) fn retention_discards(
    candidates: &[RetentionCandidate],
    max_retained: usize,
) -> Vec<(TaskId, ScheduledRunId)> {
    // The repository supplies candidates oldest-first. Keep the newest `max_retained`
    // changed worktrees, therefore discard the oldest overflow before walking the list.
    let mut remaining_overflow = candidates
        .iter()
        .filter(|candidate| {
            candidate.task_exists && !candidate.in_use && candidate.has_changes != Some(false)
        })
        .count()
        .saturating_sub(max_retained);
    let mut discards = Vec::new();
    for candidate in candidates {
        if !candidate.task_exists || candidate.in_use {
            continue;
        }
        match candidate.has_changes {
            Some(false) => discards.push((candidate.task_id.clone(), candidate.run_id.clone())),
            Some(true) | None if remaining_overflow > 0 => {
                remaining_overflow -= 1;
                discards.push((candidate.task_id.clone(), candidate.run_id.clone()));
            }
            Some(true) | None => {}
        }
    }
    discards
}

#[cfg(test)]
mod tests {
    use super::{RetentionCandidate, git_worktree_has_changes, retention_discards};
    use ora_domain::{ScheduledRunId, TaskId};
    use ora_test_support::GitTestScaffold;
    use pretty_assertions::assert_eq;

    fn candidate(
        run: &str,
        task: &str,
        exists: bool,
        has_changes: Option<bool>,
    ) -> RetentionCandidate {
        RetentionCandidate {
            run_id: ScheduledRunId::new(run),
            task_id: TaskId::new(task),
            task_exists: exists,
            in_use: false,
            has_changes,
        }
    }

    fn candidate_in_use(
        run: &str,
        task: &str,
        exists: bool,
        has_changes: Option<bool>,
    ) -> RetentionCandidate {
        RetentionCandidate {
            in_use: true,
            ..candidate(run, task, exists, has_changes)
        }
    }

    fn discards(candidates: &[RetentionCandidate], max_retained: usize) -> Vec<String> {
        retention_discards(candidates, max_retained)
            .into_iter()
            .map(|(task_id, _)| task_id.to_string())
            .collect()
    }

    #[test]
    fn discards_unmodified_worktrees_and_keeps_the_retained_bound() {
        let candidates = vec![
            candidate("run-1", "task-1", true, Some(true)),
            candidate("run-2", "task-2", true, Some(false)),
            candidate("run-3", "task-3", true, Some(true)),
            candidate("run-4", "task-4", true, None),
        ];

        assert_eq!(
            discards(&candidates, 2),
            vec!["task-1".to_string(), "task-2".to_string()]
        );
    }

    #[test]
    fn deletes_oldest_kept_worktrees_once_the_bound_is_exceeded() {
        let candidates = vec![
            candidate("run-1", "task-1", true, Some(true)),
            candidate("run-2", "task-2", true, Some(true)),
            candidate("run-3", "task-3", true, Some(true)),
        ];

        assert_eq!(discards(&candidates, 2), vec!["task-1".to_string()]);
    }

    #[test]
    fn skips_candidates_whose_task_row_disappeared() {
        let candidates = vec![
            candidate("run-1", "task-1", false, None),
            candidate("run-2", "task-2", true, Some(true)),
        ];

        assert_eq!(discards(&candidates, 2), Vec::<String>::new());
    }

    #[test]
    fn unclassifiable_worktrees_count_toward_the_bound() {
        let candidates = vec![
            candidate("run-1", "task-1", true, None),
            candidate("run-2", "task-2", true, None),
        ];

        assert_eq!(discards(&candidates, 1), vec!["task-1".to_string()]);
        assert_eq!(discards(&candidates, 2), Vec::<String>::new());
    }

    #[test]
    fn in_use_worktrees_are_neither_discarded_nor_counted_toward_the_bound() {
        let candidates = vec![
            candidate_in_use("run-1", "task-1", true, Some(false)),
            candidate_in_use("run-2", "task-2", true, Some(true)),
            candidate("run-3", "task-3", true, Some(true)),
        ];

        // In-use worktrees survive a user's live session even at bound zero; the bound
        // counts only the unused set, so the single unused retained worktree exceeds it.
        assert_eq!(discards(&candidates, 0), vec!["task-3".to_string()]);
        assert_eq!(discards(&candidates, 2), Vec::<String>::new());

        let unused_only = vec![
            candidate("run-3", "task-3", true, Some(true)),
            candidate("run-4", "task-4", true, Some(true)),
        ];
        assert_eq!(discards(&unused_only, 1), vec!["task-3".to_string()]);
    }

    /// Builds a repository with one commit and a linked worktree created from that commit.
    fn scaffold_with_worktree() -> (GitTestScaffold, std::path::PathBuf, String, String) {
        let scaffold =
            GitTestScaffold::new("scheduled-task-worktree").expect("create Git test scaffold");
        scaffold
            .write_file(scaffold.repo_path(), "README.md", "fixture")
            .expect("write file");
        scaffold
            .stage_all_and_commit("init")
            .expect("create initial commit");
        let baseline = scaffold
            .run_git(["rev-parse", "HEAD"])
            .expect("read baseline commit")
            .trim()
            .to_string();
        let checkout = scaffold
            .create_linked_worktree("run-worktree", "ora/scheduled-1")
            .expect("create linked worktree");
        let repo = scaffold.repo_path().to_path_buf();
        (
            scaffold,
            repo,
            baseline,
            checkout.to_string_lossy().into_owned(),
        )
    }

    #[test]
    fn a_fresh_worktree_against_its_recorded_baseline_has_no_changes() {
        let (_scaffold, repo, baseline, checkout) = scaffold_with_worktree();

        assert_eq!(
            git_worktree_has_changes(&repo, &checkout, Some(baseline)),
            Some(false)
        );
    }

    #[test]
    fn untracked_files_mark_a_worktree_modified() {
        let (scaffold, repo, baseline, checkout) = scaffold_with_worktree();
        scaffold
            .write_file(&checkout, "notes.txt", "agent output")
            .expect("write untracked file");

        assert_eq!(
            git_worktree_has_changes(&repo, &checkout, Some(baseline)),
            Some(true)
        );
    }

    #[test]
    fn committed_agent_changes_mark_a_worktree_modified() {
        let (scaffold, repo, baseline, checkout) = scaffold_with_worktree();
        scaffold
            .write_file(&checkout, "README.md", "agent result")
            .expect("edit tracked file");
        scaffold
            .run_git_in(&checkout, ["add", "-A"])
            .expect("stage changes");
        scaffold
            .run_git_in(&checkout, ["commit", "--no-gpg-sign", "-m", "agent change"])
            .expect("commit changes");

        assert_eq!(
            git_worktree_has_changes(&repo, &checkout, Some(baseline)),
            Some(true)
        );
    }

    #[test]
    fn a_missing_checkout_reports_no_changes() {
        let (_scaffold, repo, baseline, _checkout) = scaffold_with_worktree();

        assert_eq!(
            git_worktree_has_changes(&repo, "definitely/not/a/worktree", Some(baseline)),
            Some(false)
        );
    }

    #[test]
    fn an_unknown_baseline_keeps_a_clean_worktree_unclassifiable() {
        let (_scaffold, repo, _baseline, checkout) = scaffold_with_worktree();

        assert_eq!(
            git_worktree_has_changes(&repo, &checkout, None),
            None,
            "without a recorded baseline the worktree must be kept"
        );
    }

    /// Drives bootstrap, repository setup, and asynchronous cleanup under one scoped TRACE
    /// subscriber with a current-thread Tokio runtime.
    fn run_integration(test: impl std::future::Future<Output = ()>) {
        ora_logging::with_trace_logging(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime")
                .block_on(test);
        });
    }

    fn integration_scheduled_task(project_id: &str) -> ora_domain::ScheduledTask {
        let mut task = ora_domain::ScheduledTask::new(
            ora_domain::ScheduledTaskId::new("scheduled-task-it"),
            "Integration task",
            true,
            ora_domain::ScheduledTaskSchedule::daily(9, 0).expect("daily schedule"),
            ora_domain::ScheduledTaskExecutionLocation::Local,
            ora_domain::ProjectId::new(project_id),
            ora_domain::AgentRef::parse("ora/agent").expect("agent ref"),
            Some("default".to_string()),
            "Run the integration prompt",
            Some(ora_domain::ScheduledTaskApproval::full_auto_in_worktree(10)),
            ora_domain::AuditFields::new(10, 10, false),
        )
        .expect("task");
        task.freeze_worktree_base_branch("main");
        task
    }

    /// A finished run whose worktree is still in use keeps it; once the session stops, the
    /// unmodified worktree is discarded through the task cascade and its cleanup job is
    /// enqueued, and the run stops being tracked as retained.
    #[test]
    fn finished_run_discards_a_clean_worktree_only_after_its_session_stops() {
        run_integration(async {
            use ora_application::{
                ScheduledOccurrenceRepository, ScheduledRunLifecycleRepository,
                ScheduledTaskRepository, SessionRepository, TaskActivityRepository,
            };
            use ora_contracts::{
                CreateProjectRequest, CreateTaskRequest, EmptyErrorParams, GetTaskRequest,
                GetTaskWorkspaceRequest, PublicError,
            };
            use ora_db::{
                DatabaseBootstrapper, DatabaseLocation, SqliteGitCleanupJobRepository,
                SqliteScheduledRunRepository, SqliteScheduledTaskRepository,
                SqliteSessionRepository, SqliteTaskRepository, SqliteWorkspaceRepository,
                default_migration_catalog,
            };
            use ora_domain::{
                ScheduledOccurrence, ScheduledRun, ScheduledRunId, ScheduledRunTransition, Session,
                SessionId, SessionMcpSelection, SessionStatus, WorkspaceId,
            };
            use tempfile::TempDir;

            let temporary = TempDir::new().expect("backend fixture");
            let git = GitTestScaffold::new("scheduled-worktree-cleanup").expect("Git fixture");
            git.write_file(git.repo_path(), "README.md", "fixture\n")
                .expect("seed repository");
            git.stage_all_and_commit("initial").expect("initial commit");
            let backend = crate::Backend::open(crate::test_backend::backend_paths(
                temporary.path(),
                temporary.path(),
            ))
            .expect("backend");
            let tasks = backend.tasks();
            let project = backend
                .projects()
                .create(CreateProjectRequest {
                    name: "Scheduled cleanup fixture".to_string(),
                    main_workspace_path: git.repo_path().to_string_lossy().into_owned(),
                })
                .expect("project")
                .project;
            let worktree_task = tasks
                .create(CreateTaskRequest {
                    project_id: project.id.clone(),
                    title: "Scheduled run run-it".to_string(),
                    base_branch: Some("main".to_string()),
                })
                .expect("worktree task")
                .task;
            let worktree_root = tasks
                .workspace(GetTaskWorkspaceRequest {
                    task_id: worktree_task.id.clone(),
                })
                .expect("worktree workspace")
                .workspace
                .root_path;

            let database_path = temporary.path().join("ora.sqlite3");
            let pool = DatabaseBootstrapper::system()
                .bootstrap_repository_pool(
                    &DatabaseLocation::path(&database_path),
                    &default_migration_catalog().expect("migration catalog"),
                )
                .expect("reopen database pool");
            let definition = integration_scheduled_task(&project.id);
            SqliteScheduledTaskRepository::new(pool.clone())
                .create_scheduled_task(definition.clone())
                .expect("create definition");
            let runs = SqliteScheduledRunRepository::new(pool.clone());
            let worktree_task_id = ora_domain::TaskId::new(&worktree_task.id);
            let run = ScheduledRun::accepted(
                ScheduledRunId::new("run-it"),
                ScheduledOccurrence {
                    task_id: definition.id.clone(),
                    scheduled_at: 1_000,
                },
                &definition,
                1_001,
            );
            runs.resolve_scheduled_occurrence(ora_application::ScheduledOccurrenceCandidate::run(
                run,
            ))
            .expect("admit run");
            let session_id = SessionId::new("session-it");
            runs.transition_scheduled_run(
                &ScheduledRunId::new("run-it"),
                ScheduledRunTransition::Start {
                    session_id: session_id.clone(),
                    worktree_path: worktree_root.clone(),
                    worktree_task_id: worktree_task_id.clone(),
                },
                1_002,
            )
            .expect("start run");
            runs.transition_scheduled_run(
                &ScheduledRunId::new("run-it"),
                ScheduledRunTransition::Succeed,
                1_003,
            )
            .expect("finish run");

            let sessions = SqliteSessionRepository::new(pool.clone());
            let now = <crate::clock::SystemClock as ora_application::Clock>::now_timestamp_millis(
                &crate::clock::SystemClock,
            );
            sessions
                .create_session(Session::new(
                    session_id.clone(),
                    WorkspaceId::new(&worktree_task.workspace_id),
                    ora_domain::AgentRef::parse("ora-space.fixture").expect("agent identity"),
                    "provider-session",
                    SessionStatus::Running,
                    SessionMcpSelection::Automatic,
                    ora_domain::AuditFields::new(now, now, false),
                ))
                .expect("running session");
            assert!(
                SqliteTaskRepository::new(pool.clone())
                    .task_has_running_activity(&worktree_task_id)
                    .expect("activity probe")
            );

            let janitor = super::ScheduledWorktreeJanitor::new(
                tasks.clone(),
                pool.clone(),
                temporary.path().to_path_buf(),
            );
            let run_row = runs
                .list_scheduled_runs_with_retained_worktrees()
                .expect("list retained runs")
                .into_iter()
                .next()
                .expect("retained run row");
            janitor
                .run_finished(&run_row, &worktree_task_id, Some(&worktree_root))
                .await;
            assert!(
                tasks
                    .get(GetTaskRequest {
                        task_id: worktree_task.id.clone()
                    })
                    .is_ok(),
                "an in-use worktree is not cleaned up"
            );

            sessions
                .update_session_status(&session_id, SessionStatus::Stopped, now)
                .expect("stop the run session");
            // The cascade soft-deletes the workspace row, so capture its locator before the
            // discard; the cleanup job records the same path as its checkout root.
            let locator_path = match SqliteWorkspaceRepository::new(pool.clone())
                .find_workspace(&WorkspaceId::new(&worktree_task.workspace_id))
                .expect("workspace lookup")
                .map(|workspace| workspace.location)
            {
                Some(ora_domain::WorkspaceLocation::LocalFilesystem { path }) => path,
                _ => panic!("the retained worktree task owns a local filesystem workspace"),
            };
            janitor
                .run_finished(&run_row, &worktree_task_id, Some(&worktree_root))
                .await;

            let delete_error = tasks
                .get(GetTaskRequest {
                    task_id: worktree_task.id.clone(),
                })
                .expect_err("the clean worktree task is deleted");
            assert_eq!(
                delete_error.public_error(),
                &PublicError::TaskNotFound(EmptyErrorParams {})
            );
            assert!(
                runs.list_scheduled_runs_with_retained_worktrees()
                    .expect("list retained runs after discard")
                    .is_empty(),
                "the discarded run no longer counts as retained"
            );
            let cleanup_jobs = SqliteGitCleanupJobRepository::new(pool)
                .list_jobs()
                .expect("list cleanup jobs");
            assert!(
                cleanup_jobs
                    .iter()
                    .any(|job| job.checkout_root.as_deref() == Some(locator_path.as_str())),
                "the worktree cleanup is enqueued for the durable worker"
            );
        });
    }

    /// A sweep unlinks a run whose worktree task row was deleted through the ordinary Task
    /// API, so the run becomes prunable history again; the unlink only happens on a proven
    /// missing row, never on a probe failure.
    #[test]
    fn retention_sweep_unlinks_a_run_whose_worktree_task_was_deleted() {
        run_integration(async {
            use ora_application::{
                ScheduledOccurrenceRepository, ScheduledRunLifecycleRepository,
                ScheduledTaskHistoryRepository, ScheduledTaskRepository, TaskRepository,
            };
            use ora_contracts::{CreateProjectRequest, CreateTaskRequest, DeleteTaskRequest};
            use ora_db::{
                DatabaseBootstrapper, DatabaseLocation, SqliteScheduledRunRepository,
                SqliteScheduledTaskRepository, default_migration_catalog,
            };
            use ora_domain::{
                ScheduledMissReason, ScheduledMissedOccurrence, ScheduledOccurrence, ScheduledRun,
                ScheduledRunId, ScheduledRunTransition,
            };
            use tempfile::TempDir;

            let temporary = TempDir::new().expect("backend fixture");
            let git = GitTestScaffold::new("scheduled-worktree-unlink").expect("Git fixture");
            git.write_file(git.repo_path(), "README.md", "fixture\n")
                .expect("seed repository");
            git.stage_all_and_commit("initial").expect("initial commit");
            let backend = crate::Backend::open(crate::test_backend::backend_paths(
                temporary.path(),
                temporary.path(),
            ))
            .expect("backend");
            let tasks = backend.tasks();
            let project = backend
                .projects()
                .create(CreateProjectRequest {
                    name: "Scheduled unlink fixture".to_string(),
                    main_workspace_path: git.repo_path().to_string_lossy().into_owned(),
                })
                .expect("project")
                .project;
            let worktree_task = tasks
                .create(CreateTaskRequest {
                    project_id: project.id.clone(),
                    title: "Scheduled run run-unlink".to_string(),
                    base_branch: Some("main".to_string()),
                })
                .expect("worktree task")
                .task;
            let worktree_root = tasks
                .workspace(ora_contracts::GetTaskWorkspaceRequest {
                    task_id: worktree_task.id.clone(),
                })
                .expect("worktree workspace")
                .workspace
                .root_path;

            let database_path = temporary.path().join("ora.sqlite3");
            let pool = DatabaseBootstrapper::system()
                .bootstrap_repository_pool(
                    &DatabaseLocation::path(&database_path),
                    &default_migration_catalog().expect("migration catalog"),
                )
                .expect("reopen database pool");
            let definition = integration_scheduled_task(&project.id);
            SqliteScheduledTaskRepository::new(pool.clone())
                .create_scheduled_task(definition.clone())
                .expect("create definition");
            let runs = SqliteScheduledRunRepository::new(pool.clone());
            let worktree_task_id = ora_domain::TaskId::new(&worktree_task.id);
            let run = ScheduledRun::accepted(
                ScheduledRunId::new("run-unlink"),
                ScheduledOccurrence {
                    task_id: definition.id.clone(),
                    scheduled_at: 1_000,
                },
                &definition,
                1_001,
            );
            runs.resolve_scheduled_occurrence(ora_application::ScheduledOccurrenceCandidate::run(
                run,
            ))
            .expect("admit run");
            runs.transition_scheduled_run(
                &ScheduledRunId::new("run-unlink"),
                ScheduledRunTransition::Start {
                    session_id: ora_domain::SessionId::new("session-unlink"),
                    worktree_path: worktree_root.clone(),
                    worktree_task_id: worktree_task_id.clone(),
                },
                1_002,
            )
            .expect("start run");
            runs.transition_scheduled_run(
                &ScheduledRunId::new("run-unlink"),
                ScheduledRunTransition::Succeed,
                1_003,
            )
            .expect("finish run");

            // The user deletes the retained worktree through the ordinary task list.
            tasks
                .delete(DeleteTaskRequest {
                    task_id: worktree_task.id.clone(),
                })
                .await
                .expect("delete the worktree task");

            let run_row = runs
                .list_scheduled_runs_with_retained_worktrees()
                .expect("list retained runs")
                .into_iter()
                .next()
                .expect("the deleted task's run is still tracked");
            assert_eq!(run_row.id.as_ref(), "run-unlink");

            super::ScheduledWorktreeJanitor::new(
                tasks.clone(),
                pool.clone(),
                temporary.path().to_path_buf(),
            )
            .enforce_retention()
            .await;

            assert!(
                runs.list_scheduled_runs_with_retained_worktrees()
                    .expect("list retained runs after sweep")
                    .is_empty(),
                "the stale link is cleared once the task row is proven gone"
            );

            // History pruning resumes: pushing past the bound removes the unlinked run.
            for scheduled_at in 2_000_i64..=2_100 {
                runs.resolve_scheduled_occurrence(
                    ora_application::ScheduledOccurrenceCandidate::Missed(
                        ScheduledMissedOccurrence {
                            occurrence: ScheduledOccurrence {
                                task_id: definition.id.clone(),
                                scheduled_at,
                            },
                            reason: ScheduledMissReason::AppNotRunning,
                            observed_at: scheduled_at,
                        },
                    ),
                )
                .expect("persist missed facts past the bound");
            }
            let prunable_left: Vec<_> = runs
                .list_scheduled_task_history(&definition.id)
                .expect("list history after pruning")
                .into_iter()
                .filter(|entry| {
                    matches!(entry, ora_domain::ScheduledTaskHistoryEntry::Run(run) if run.id.as_ref() == "run-unlink")
                })
                .collect();
            assert!(
                prunable_left.is_empty(),
                "the unlinked run is pruned like any other history fact"
            );
        });
    }
}
