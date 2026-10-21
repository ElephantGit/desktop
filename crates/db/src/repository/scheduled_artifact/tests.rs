use ora_application::ScheduledRunArtifactRepository;
use ora_domain::{
    ScheduledArtifactKind, ScheduledArtifactRejection, ScheduledArtifactRejectionReason,
    ScheduledRunArtifact, ScheduledRunId,
};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::{
    DatabaseBootstrapper, DatabaseLocation, RepositoryPool, default_migration_catalog,
    repository::SqliteScheduledRunRepository, test_clock::TestClock,
};

fn bootstrapped_pool() -> (TempDir, RepositoryPool) {
    let temporary = TempDir::new().expect("create temp directory");
    let database_path = temporary.path().join("scheduled-artifact.sqlite3");
    let pool = DatabaseBootstrapper::new(TestClock::new(1))
        .bootstrap_repository_pool(
            &DatabaseLocation::path(database_path),
            &default_migration_catalog().expect("build migration catalog"),
        )
        .expect("bootstrap database");
    (temporary, pool)
}

fn artifact(name: &str, size: u64, registered_at: i64) -> ScheduledRunArtifact {
    ScheduledRunArtifact {
        relative_path: name.to_string(),
        kind: ScheduledArtifactKind::Markdown,
        size_bytes: size,
        registered_at,
    }
}

#[test]
fn records_lists_finds_and_deletes_a_run_manifest() {
    let (_temporary, pool) = bootstrapped_pool();
    let repository = SqliteScheduledRunRepository::new(pool);
    let run_id = ScheduledRunId::new("run-1");
    repository
        .record_scheduled_run_artifacts(
            &run_id,
            &[
                artifact("first.md", 5, 10),
                artifact("nested/second.json", 6, 11),
            ],
            &[ScheduledArtifactRejection {
                name: "secret.pdf".to_string(),
                reason: ScheduledArtifactRejectionReason::UnsupportedType,
            }],
            10,
        )
        .expect("record artifacts");

    assert_eq!(
        repository
            .list_scheduled_run_artifacts(&run_id)
            .expect("list artifacts"),
        vec![
            artifact("first.md", 5, 10),
            artifact("nested/second.json", 6, 11)
        ]
    );
    assert_eq!(
        repository
            .find_scheduled_run_artifact(&run_id, "nested/second.json")
            .expect("find artifact"),
        Some(artifact("nested/second.json", 6, 11))
    );
    assert_eq!(
        repository
            .list_scheduled_run_artifact_rejections(&run_id)
            .expect("list rejections"),
        vec![ScheduledArtifactRejection {
            name: "secret.pdf".to_string(),
            reason: ScheduledArtifactRejectionReason::UnsupportedType,
        }]
    );
    assert!(
        repository
            .delete_scheduled_run_artifact(&run_id, "first.md")
            .expect("delete artifact")
    );
    assert_eq!(
        repository
            .list_scheduled_run_artifacts(&run_id)
            .expect("list after delete"),
        vec![artifact("nested/second.json", 6, 11)]
    );
    assert_eq!(
        repository
            .delete_scheduled_run_artifact_rows(&run_id)
            .expect("delete rows"),
        2
    );
    assert!(
        repository
            .find_scheduled_artifact_run(&run_id)
            .expect("find subject")
            .is_none()
    );
}

#[test]
fn pruned_runs_surface_as_missing_so_the_sweep_can_remove_their_artifacts() {
    let (_temporary, pool) = bootstrapped_pool();
    let repository = SqliteScheduledRunRepository::new(pool);
    let run_id = ScheduledRunId::new("run-pruned");
    repository
        .record_scheduled_run_artifacts(&run_id, &[artifact("report.md", 6, 1)], &[], 1)
        .expect("record artifacts");

    assert_eq!(
        repository
            .find_scheduled_artifact_run(&run_id)
            .expect("find subject"),
        Some(ora_application::ScheduledArtifactRunRecord {
            run_id: run_id.clone(),
            task_id: None,
            updated_at: None,
            run_missing: true,
            task_deleted: true,
            retains_worktree: false,
        })
    );
    assert_eq!(
        repository
            .list_scheduled_artifact_runs()
            .expect("list artifact runs"),
        vec![ora_application::ScheduledArtifactRunRecord {
            run_id,
            task_id: None,
            updated_at: None,
            run_missing: true,
            task_deleted: true,
            retains_worktree: false,
        }]
    );
}
