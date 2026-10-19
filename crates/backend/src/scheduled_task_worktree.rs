use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitlancer::git::diff::{DiffRequest, DiffScope};
use gitlancer::git::status::StatusEntriesRequest;
use gitlancer::git::worktree::FindWorktreeRequest;
use gitlancer::{CliGitRunner, CommitId, Git, RepoRoot, Repository};
use ora_application::{ScheduledRunLifecycleRepository, TaskRepository, WorktreeRepository};
use ora_contracts::DeleteTaskRequest;
use ora_db::{
    SqliteScheduledRunRepository, SqliteTaskRepository, SqliteWorkspaceRepository,
    SqliteWorktreeRepository,
};
use ora_domain::{ProjectId, ScheduledRun, ScheduledRunId, TaskId, WorkspaceLocation};

use crate::TaskApi;

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
/// runs deleted first.
#[derive(Clone)]
pub(super) struct ScheduledWorktreeJanitor {
    tasks: Arc<TaskApi>,
    runs: SqliteScheduledRunRepository,
    task_rows: SqliteTaskRepository,
    workspaces: SqliteWorkspaceRepository,
    worktrees: SqliteWorktreeRepository,
}

impl ScheduledWorktreeJanitor {
    pub(super) fn new(tasks: Arc<TaskApi>, pool: ora_db::RepositoryPool) -> Self {
        Self {
            tasks,
            runs: SqliteScheduledRunRepository::new(pool.clone()),
            task_rows: SqliteTaskRepository::new(pool.clone()),
            workspaces: SqliteWorkspaceRepository::new(pool.clone()),
            worktrees: SqliteWorktreeRepository::new(pool),
        }
    }

    /// Decides keep-versus-delete for one finished run, then keeps the retention bound.
    ///
    /// A run that never started still owns a freshly provisioned worktree; it cannot carry
    /// changes and is discarded directly.
    pub(super) async fn run_finished(
        &self,
        run: &ScheduledRun,
        worktree_task_id: &TaskId,
        worktree_path: Option<&str>,
    ) {
        if !self.task_exists(worktree_task_id) {
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
    /// covers runs a previous process finished without cleanup.
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
            let task_exists = self.task_exists(&task_id);
            let has_changes = if task_exists {
                self.classify_worktree(&run, &task_id, &worktree_path)
            } else {
                Some(false)
            };
            candidates.push(RetentionCandidate {
                run_id: run.id.clone(),
                task_id,
                task_exists,
                has_changes,
            });
        }
        for (task_id, run_id) in retention_discards(&candidates, MAX_RETAINED_SCHEDULED_WORKTREES) {
            self.discard_task(&task_id, &run_id).await;
        }
    }

    fn task_exists(&self, task_id: &TaskId) -> bool {
        self.task_rows
            .find_task(task_id)
            .map(|task| task.is_some())
            .unwrap_or(false)
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
        let repository_root = self.project_repository_root(&run.snapshot.project_id)?;
        git_worktree_has_changes(&repository_root, worktree_path, baseline)
    }

    fn project_repository_root(&self, project_id: &ProjectId) -> Option<PathBuf> {
        let workspace = self.workspaces.find_main_workspace(project_id).ok()??;
        match workspace.location {
            WorkspaceLocation::LocalFilesystem { path } => Some(PathBuf::from(path)),
            _ => None,
        }
    }

    /// Discards one run's retained worktree through the regular task cascade.
    async fn discard_task(&self, task_id: &TaskId, run_id: &ScheduledRunId) {
        if let Err(error) = self
            .tasks
            .clone()
            .delete(DeleteTaskRequest {
                task_id: task_id.to_string(),
            })
            .await
        {
            ora_logging::ora_warn!(
                run_id = %run_id,
                task_id = %task_id,
                error = %error,
                "scheduled run worktree discard failed; the worktree is kept"
            );
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
/// task row already disappeared are skipped because nothing remains to discard.
pub(super) fn retention_discards(
    candidates: &[RetentionCandidate],
    max_retained: usize,
) -> Vec<(TaskId, ScheduledRunId)> {
    let mut retained = 0usize;
    let mut discards = Vec::new();
    for candidate in candidates {
        if !candidate.task_exists {
            continue;
        }
        match candidate.has_changes {
            Some(false) => discards.push((candidate.task_id.clone(), candidate.run_id.clone())),
            Some(true) | None => {
                retained += 1;
                if retained > max_retained {
                    discards.push((candidate.task_id.clone(), candidate.run_id.clone()));
                }
            }
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
            has_changes,
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
            vec!["task-2".to_string(), "task-4".to_string()]
        );
    }

    #[test]
    fn deletes_oldest_kept_worktrees_once_the_bound_is_exceeded() {
        let candidates = vec![
            candidate("run-1", "task-1", true, Some(true)),
            candidate("run-2", "task-2", true, Some(true)),
            candidate("run-3", "task-3", true, Some(true)),
        ];

        assert_eq!(discards(&candidates, 2), vec!["task-3".to_string()]);
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

        assert_eq!(discards(&candidates, 1), vec!["task-2".to_string()]);
        assert_eq!(discards(&candidates, 2), Vec::<String>::new());
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
}
