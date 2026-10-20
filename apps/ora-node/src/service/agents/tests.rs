use crate::{AgentConfig, ProcessConfig, ServiceConfig, SessionWorkload, Shutdown};
use ora_process::ProcessIdentity;
use pretty_assertions::assert_eq;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A service configuration over `root` that would run agents as `workload_uid` from
/// `workload_directory`; everything else is valid and never reached by these tests.
fn config(
    root: &Path,
    workload_uid: Option<u32>,
    workload_directory: Option<PathBuf>,
) -> ServiceConfig {
    let mut config: ServiceConfig = serde_json::from_value(serde_json::json!({
        "node": {
            "home_directory": root.join("node"),
            "identity": "Discover",
            "repositories": [],
        },
        "process": {
            "host_directory": root.join("host"),
            "expected_uid": 0,
            "git_program": "/usr/bin/git",
            "environment": {},
            "command_timeout_ms": 1000,
            "cleanup_timeout_ms": 1000,
            "shutdown_grace_ms": 1000,
        },
        "recovery_interval_ms": 50,
        "timezone": "UTC",
    }))
    .expect("service configuration");
    config.process.workload_uid = workload_uid;
    config.agent = Some(AgentConfig {
        model_proxy: None,
        deno_path: PathBuf::from("/usr/bin/deno"),
        ready_timeout_ms: 1000,
        workload_directory,
        ora_git_directory: None,
    });
    config
}

/// Creates `path` as a directory with exactly `mode`.
fn directory(path: &Path, mode: u32) -> PathBuf {
    fs::create_dir_all(path).expect("create directory");
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set mode");
    path.to_path_buf()
}

/// Startup keeps unknown restore inputs; local cleanup failure never rewrites stop evidence.
#[test]
fn restore_cleanup_requires_original_terminal_stop_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    use ora_node_db::{NodeIdentity, ProcessAttempt};
    use ora_node_protocol::*;
    use ora_process_protocol::*;
    ora_logging::initialize_test_clock();
    let subscriber = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish(),
    );
    tracing::dispatcher::with_default(&subscriber, || {
        let root = tempfile::tempdir()?;
        let mut deployment = config(
            root.path(),
            /*workload_uid*/ None,
            /*workload_directory*/ None,
        );
        deployment.node.identity = NodeIdentity::Require(NodeId::new("node"));
        let mut node = crate::Node::open(deployment.node, deployment.process, Shutdown::default())?;
        let input: StartAgentSessionMessage = serde_json::from_value(serde_json::json!({
            "protocol_version": CURRENT_PROTOCOL_VERSION, "operation_id":"restore-op", "execution_id":"restore-session",
            "payload":{"spec":{
                "node_id":"node", "agent_plugin_id":"official/agent", "agent_plugin_version":"1.0.0", "checkout_execution_id":"clone",
                "git_identity":{"name":"Fixture","email":"fixture@example.test"},
                "initial_turn":{"turn_id":"initial","content":[{"type":"text","text":"fixture"}]},
                "prior_revision":{"revision_id":"prior","final_commit":"0123456789012345678901234567890123456789","bundle":{"key":"prior.bundle","size":1,"sha256":"0000000000000000000000000000000000000000000000000000000000000000"}}
            }}
        }))?;
        node.database.accept_session(&input)?;
        node.database
            .start_session(&input, &NodeIncarnationId::new("original"))?;
        let journal = node.database.session_journal()?;
        let attempt = ProcessAttempt {
            execution: input.execution_id.clone(),
            host_directory: root.path().join("original-host"),
            expected_uid: 42,
            intent: HostRunIntent {
                scope: ScopeId::new(),
                run: RunId::new(),
                spec: RunSpec::new("git", root.path(), DescendantPolicy::WaitForAll),
                host_disconnect: GuardianHostDisconnect::KeepRunning,
            },
        };
        journal.record_restore_attempt(&attempt, StopContainment::BestEffort)?;
        let directory = node
            .home_directory()
            .join(crate::revision::RESTORE_ROOT)
            .join(ora_utils::hash::sha256_hex(
                input.execution_id.as_str().as_bytes(),
            ));
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("prior.bundle"), "original process input")?;
        super::sweep_restores(&node, &journal)?;
        assert!(directory.join("prior.bundle").is_file());
        let ended = AgentSessionEnded {
            node: node.identity().clone(),
            reason: AgentSessionEndReason::Interrupted,
            detail: None,
        };
        assert!(
            journal
                .end_session(&input.execution_id, ended.clone())
                .is_err()
        );
        // The process monitor alone acknowledges its independently verified original proof.
        journal.confirm_restore_stop(attempt.intent.run)?;
        journal.end_session(&input.execution_id, ended)?;
        super::sweep_restores(&node, &journal)?;
        assert!(!directory.exists());
        fs::write(&directory, "unexpected local residue")?;
        super::sweep_restores(&node, &journal)?;
        assert!(directory.is_file());
        assert!(journal.session_completed(&input.execution_id)?);
        assert!(
            journal
                .pending_restore_attempts(&input.execution_id)?
                .is_empty()
        );
        Ok(())
    })
}

/// Agents run as the workload user exactly when Git does, from a directory only the Node may
/// change that overlaps none of its state; `serve` refuses every other combination before it
/// opens anything.
#[tokio::test]
async fn serve_refuses_an_agent_workload_that_does_not_match_the_process_workload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let valid = directory(&root.join("agent"), 0o711);
    let shared = directory(&root.join("shared"), 0o777);
    let inside_home = directory(&root.join("node").join("agent"), 0o711);
    let cases = [
        config(root, Some(1000), None),
        config(root, None, Some(valid.clone())),
        config(root, Some(1000), Some(PathBuf::from("relative"))),
        config(root, Some(1000), Some(root.join("missing"))),
        config(root, Some(1000), Some(shared)),
        config(root, Some(1000), Some(inside_home)),
    ];

    let mut errors = Vec::new();
    for case in cases {
        errors.push(
            crate::serve(case, Shutdown::default())
                .await
                .expect_err("the configuration is refused")
                .to_string(),
        );
    }

    let mismatch = "agent workload_directory is required exactly when process.workload_uid is set";
    assert_eq!(
        errors,
        vec![
            mismatch.to_string(),
            mismatch.to_string(),
            "agent workload directory must be an absolute UTF-8 path without parent traversal"
                .to_string(),
            "No such file or directory (os error 2)".to_string(),
            "agent workload directory must belong to the Node identity and be writable by no one else"
                .to_string(),
            "agent workload directory overlaps Node state, host state or the clone root"
                .to_string(),
        ]
    );
}

/// The `ora-git` directory is the Node's alone: an absolute path that is missing (the Node creates
/// it) or a Node-owned directory nobody else can list or change, overlapping none of the
/// directories the Node manages; `serve` refuses anything else before it removes any socket.
#[tokio::test]
async fn serve_refuses_an_ora_git_directory_that_is_not_the_nodes_alone() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    // The tempdir itself is one of the cases: Node-owned and private, so only the overlap refuses it.
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).expect("set mode");
    let with = |ora_git: PathBuf| {
        let mut config = config(root, None, None);
        config.agent.as_mut().expect("agent").ora_git_directory = Some(ora_git);
        config
    };
    let shared = directory(&root.join("shared"), 0o755);
    let home = directory(&root.join("node"), 0o700);
    let mut workload = config(
        root,
        Some(1000),
        Some(directory(&root.join("agent"), 0o711)),
    );
    workload.agent.as_mut().expect("agent").ora_git_directory = Some(root.join("agent/ora-git"));
    let cases = [
        with(PathBuf::from("relative")),
        with(PathBuf::from("/")),
        with(shared),
        with(root.join("missing/ora-git")),
        with(root.to_path_buf()),
        with(home.join("ora-git")),
        workload,
    ];

    let mut errors = Vec::new();
    for case in cases {
        errors.push(
            super::validate(case.agent.as_ref().expect("agent"), &case)
                .expect_err("the configuration is refused")
                .to_string(),
        );
    }

    let overlap = "agent ora_git_directory overlaps Node state, host state, the clone root or the workload directory";
    assert_eq!(
        errors,
        vec![
            "agent ora_git_directory must be an absolute UTF-8 path without parent traversal, below the root",
            "agent ora_git_directory must be an absolute UTF-8 path without parent traversal, below the root",
            "agent ora_git_directory must belong to the Node identity and be readable and writable by no one else",
            "agent ora_git_directory must be created under a directory only trusted owners control",
            overlap,
            overlap,
            overlap,
        ]
    );
    for accepted in [
        root.join("ora-git"),
        directory(&root.join("sockets"), 0o711),
    ] {
        let case = with(accepted);
        super::validate(case.agent.as_ref().expect("agent"), &case).expect("accepted");
    }
}

/// Without a workload user agents share the Node identity; with one they take exactly that user,
/// with a group equal to it.
#[test]
fn the_workload_follows_the_process_workload_user() {
    let temp = tempfile::tempdir().expect("tempdir");
    let directory = temp.path().join("agent");
    let shared = config(temp.path(), None, None);
    let separate = config(temp.path(), Some(1000), Some(directory.clone()));
    let process = |config: &ServiceConfig| -> ProcessConfig { config.process.clone() };

    assert_eq!(
        (
            super::workload(shared.agent.as_ref(), &process(&shared)).expect("shared"),
            super::workload(separate.agent.as_ref(), &process(&separate)).expect("separate"),
        ),
        (
            SessionWorkload::Shared,
            SessionWorkload::Separate {
                directory,
                identity: ProcessIdentity::Linux(
                    ora_utils::process::LinuxChildIdentity::new(1000, 1000).expect("identity")
                ),
                uid: 1000,
                gid: 1000,
            },
        )
    );
}

/// An invalid restore budget is rejected before opening state, even without Agent admission.
#[tokio::test]
async fn serve_refuses_zero_restore_budget_before_opening_node_state()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let mut config = config(
        temporary.path(),
        /*workload_uid*/ None,
        /*workload_directory*/ None,
    );
    config.restore.deadline_ms = 0;
    let home = config.node.home_directory.clone();
    let error = crate::serve(config, Shutdown::default())
        .await
        .err()
        .ok_or("invalid restore configuration was accepted")?;
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!home.exists());
    Ok(())
}
