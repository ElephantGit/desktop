use super::*;
use pretty_assertions::assert_eq;

/// A non-directory ancestor is not proof of absence for either recovery outcome.
#[test]
fn recovery_refuses_file_roots_without_closing_the_journal() {
    for depth in 1..=3 {
        let (directory, pool, workspace) = fixture();
        let manifest = b"---\nname: review\ndescription: Reviews changes\n---\ncontent\n";
        let (repository, clock, target, _) =
            converged_target(directory.path(), &pool, &workspace, manifest);
        rewind_to_applied_leftover(&pool, &target);
        repository
            .quarantine_unfinished_operations(LocalTimestamp::from_millis(200))
            .expect("quarantine leftover");
        let journal = repository.load_target_recovery(&target).unwrap().unwrap();
        let operation = &journal.attempts[0].operations[0];
        let delete = deletion_of(operation);
        let replaced = ["workspace", ".agents", "skills"]
            .iter()
            .take(depth)
            .fold(directory.path().to_path_buf(), |path, segment| {
                path.join(segment)
            });
        std::fs::remove_dir_all(&replaced).unwrap();
        std::fs::write(&replaced, b"user file").unwrap();
        let before = quarantine_state(&pool, &target);

        assert!(
            SkillDirectoryResourceAdapter.verify(&delete).is_err(),
            "{replaced:?} must not prove planned Missing"
        );
        assert!(
            SkillDirectoryResourceAdapter
                .verify_expected(operation)
                .is_err(),
            "{replaced:?} must not prove expected Missing"
        );
        assert!(matches!(
            recover_target(&repository, &clock, &target),
            Err(RecoveryError::NotRecoverable { .. })
        ));
        assert_eq!(quarantine_state(&pool, &target), before);
        assert_eq!(std::fs::read(&replaced).unwrap(), b"user file");
    }
}

/// Safe absence is provable without creating any missing Resource directories.
#[test]
fn recovery_proves_missing_resource_without_creating_directories() {
    let (directory, pool, workspace) = fixture();
    let manifest = b"---\nname: review\ndescription: Reviews changes\n---\ncontent\n";
    let (repository, clock, target, _) =
        converged_target(directory.path(), &pool, &workspace, manifest);
    rewind_to_applied_leftover(&pool, &target);
    repository
        .quarantine_unfinished_operations(LocalTimestamp::from_millis(200))
        .unwrap();
    let journal = repository.load_target_recovery(&target).unwrap().unwrap();
    let operation = &journal.attempts[0].operations[0];
    let root = directory.path().join("workspace").join(".agents");
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        SkillDirectoryResourceAdapter
            .verify(&deletion_of(operation))
            .is_ok()
    );
    assert!(
        SkillDirectoryResourceAdapter
            .verify_expected(operation)
            .is_ok()
    );
    assert!(recover_target(&repository, &clock, &target).is_ok());
    assert!(!root.exists(), "verification must not create directories");
}

/// Links cannot redirect exact-state proof, and dangling links cannot masquerade as absence.
#[cfg(unix)]
#[test]
fn recovery_refuses_link_roots_and_dangling_item_links() {
    for depth in 1..=4 {
        let (directory, pool, workspace) = fixture();
        let manifest = b"---\nname: review\ndescription: Reviews changes\n---\ncontent\n";
        let (repository, clock, target, _) =
            converged_target(directory.path(), &pool, &workspace, manifest);
        rewind_to_applied_leftover(&pool, &target);
        repository
            .quarantine_unfinished_operations(LocalTimestamp::from_millis(200))
            .unwrap();
        let journal = repository.load_target_recovery(&target).unwrap().unwrap();
        let operation = &journal.attempts[0].operations[0];
        let delete = deletion_of(operation);
        let replaced = ["workspace", ".agents", "skills", "review"]
            .iter()
            .take(depth)
            .fold(directory.path().to_path_buf(), |path, segment| {
                path.join(segment)
            });
        let outside = directory.path().join("outside");
        std::fs::rename(&replaced, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &replaced).unwrap();
        let before = quarantine_state(&pool, &target);
        assert!(SkillDirectoryResourceAdapter.verify(operation).is_err());
        assert!(
            SkillDirectoryResourceAdapter
                .verify_expected(&delete)
                .is_err()
        );
        // A broken link is still an entry, never proof of an exact Missing baseline.
        std::fs::remove_dir_all(&outside).unwrap();
        assert!(SkillDirectoryResourceAdapter.verify(&delete).is_err());
        assert!(
            SkillDirectoryResourceAdapter
                .verify_expected(operation)
                .is_err()
        );
        assert!(recover_target(&repository, &clock, &target).is_err());
        assert_eq!(quarantine_state(&pool, &target), before);
        assert!(
            std::fs::symlink_metadata(&replaced)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        std::fs::remove_file(&replaced).unwrap();
    }
}

/// Builds a deletion journal with the same persisted path authority as a real create.
fn deletion_of(operation: &EffectOperation) -> EffectOperation {
    let ExactPlannedState::Present {
        native_identity,
        fingerprint,
        managed_identity,
    } = operation.planned()
    else {
        panic!("fixture must create a Skill");
    };
    EffectOperation::prepare(
        EffectOperationId::new("delete-proof"),
        EffectOperationIntent {
            attempt: operation.attempt().clone(),
            resource: operation.resource().clone(),
            generation: operation.generation(),
            sequence: 0,
            mutation: EffectMutation::Delete,
            expected: ExactPreviousState::Present {
                native_identity: native_identity.clone(),
                fingerprint: fingerprint.clone(),
                managed_identity: managed_identity.clone(),
            },
            planned: ExactPlannedState::Missing,
            payload: operation.payload().clone(),
        },
        LocalTimestamp::from_millis(120),
    )
    .unwrap()
}
