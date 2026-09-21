use super::*;
use crate::support::until;
use ora_contracts::controller_api::*;
use ora_controller::{
    ApiConfig, DeploymentConfig, NodeEndpoint, NodeHosting, RuntimeConfig, SessionConfig,
    SingleNodeConfig,
};
use ora_utils::process::{LinuxPidFd, ProcessSignal, linux_process_snapshot};
use pretty_assertions::assert_eq;
use std::time::Duration;

/// Pins the Node the Controller started; test failure must not leave it listening in a deleted root.
struct HostedNode(LinuxPidFd);
impl Drop for HostedNode {
    fn drop(&mut self) {
        let _ = self.0.signal(ProcessSignal::Kill);
    }
}

/// Finds the one Node executable started from this fixture's IPC configuration, never by remembered PID.
fn hosted_node(config: &std::path::Path) -> HostedNode {
    let mut handle = None;
    until(|| {
        handle = linux_process_snapshot()
            .unwrap()
            .flatten()
            .find_map(|stat| {
                let proc = std::path::Path::new("/proc").join(stat.pid.to_string());
                let cmdline = fs::read(proc.join("cmdline")).unwrap_or_default();
                (fs::read_link(proc.join("exe")).ok().as_deref()
                    == Some(std::path::Path::new(env!("CARGO_BIN_EXE_ora-node")))
                    && cmdline
                        .split(|byte| *byte == 0)
                        .any(|arg| arg == config.as_os_str().as_encoded_bytes()))
                .then(|| LinuxPidFd::from_observation(&stat).ok())
                .flatten()
            });
        handle.is_some()
    });
    HostedNode(handle.unwrap())
}

/// The composed executable hosts its Node: Controller death leaves Node and Git running, a live
/// endpoint refuses a second hosting Controller, and normal stop retires the API, sessions and Node in order.
#[test]
fn single_node_composition_hosts_and_retires_its_node() {
    ora_logging::with_trace_logging(|| {
        let fixture = Fixture::new();
        fixture.git(&["update-server-info"]);
        let expected_commit = fixture.git(&["rev-parse", "main"]);
        let source = HttpsRepository::new(fixture.path(), fixture.path().join("main").join(".git"));
        source.paused.store(true, Ordering::SeqCst);
        let clone = configuration(&fixture, &source);
        let node_config = ipc::write_config(&fixture, &clone, /*frame_timeout_ms*/ 40_000);
        let endpoint = fixture.config().home_directory.join("control.sock");
        let config = DeploymentConfig {
            api: ApiConfig {
                node_id: NodeId::new("test-node"),
            },
            single_node: Some(SingleNodeConfig {
                node_executable: env!("CARGO_BIN_EXE_ora-node").into(),
                node_config: node_config.clone(),
                ready_timeout_ms: 15_000,
                stop_timeout_ms: 15_000,
            }),
            controller: RuntimeConfig {
                home_directory: fixture.path().join("controller"),
                protected_state_directories: vec![
                    fixture.config().home_directory,
                    fixture.process().host_directory,
                ],
                controller_id: ControllerId::new("owner"),
                nodes: vec![NodeEndpoint {
                    node_id: NodeId::new("test-node"),
                    endpoint: endpoint.clone(),
                }],
                session: SessionConfig {
                    io_timeout_ms: 5000,
                    query_interval_ms: 100,
                },
                reconnect_ms: 100,
                timezone: "Asia/Shanghai".into(),
            },
        };
        // A hosting Controller refuses to start when Node configuration binds another owner.
        let mut foreign: serde_json::Value =
            serde_json::from_slice(&fs::read(&node_config).unwrap()).unwrap();
        foreign["ipc"]["controller_id"] = "someone-else".into();
        let foreign_path = fixture.path().join("foreign-node.json");
        fs::write(&foreign_path, serde_json::to_vec(&foreign).unwrap()).unwrap();
        let mut misbound = config.clone();
        misbound.single_node.as_mut().unwrap().node_config = foreign_path;
        let (mut rejected, _) = launch_expecting_exit(&fixture, &misbound);
        assert!(!rejected.0.wait().unwrap().success());
        assert!(!fixture.path().join("controller").exists());

        let (mut controller, address) =
            minicloud::launch(&fixture, &config, /*port*/ 0, NodeHosting::Managed);
        let port: u16 = address.rsplit(':').next().unwrap().parse().unwrap();
        let first = hosted_node(&node_config);
        let execution = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let client = reqwest::Client::builder()
                    .pool_max_idle_per_host(/*max*/ 0)
                    .build()
                    .unwrap();
                let clones = format!("http://{address}/api/clones");
                let input = MiniCloneRequest {
                    request_id: "hosted".into(),
                    repository: source.address.clone(),
                    branch: "main".into(),
                };
                let receipt: MiniCloneAccepted = client
                    .post(&clones)
                    .json(&input)
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                until(|| {
                    fs::read_dir(&clone.repository_root)
                        .unwrap()
                        .flatten()
                        .any(|entry| entry.path().join(".git").is_dir())
                });
                // Killing only the Controller must not signal the Node it started or the Git it accepted.
                controller.kill();
                assert!(!first.0.has_exited().unwrap());
                let (mut refused, _) = launch_expecting_exit(&fixture, &config);
                assert!(!refused.0.wait().unwrap().success());
                assert!(!first.0.has_exited().unwrap());
                source.paused.store(false, Ordering::SeqCst);
                // The surviving Node finishes the clone on its own and replays the unacknowledged
                // result to whoever holds the owner session; observing it without an Ack leaves the
                // event for the replacement Controller to take over durably.
                tokio::time::timeout(Duration::from_secs(/*secs*/ 40), async {
                    let hello = ControllerToNodeMessage::Hello(HelloMessage {
                        protocol_version: CURRENT_PROTOCOL_VERSION,
                        payload: Hello {
                            controller_id: ControllerId::new("owner"),
                            supported_versions: vec![CURRENT_PROTOCOL_VERSION],
                        },
                    });
                    loop {
                        // The dead Controller's session is revoked asynchronously; until then the
                        // Node closes extra connections, which surfaces as a failed write or EOF.
                        tokio::time::sleep(Duration::from_millis(/*millis*/ 50)).await;
                        let Ok(mut stream) = tokio::net::UnixStream::connect(&endpoint).await
                        else {
                            continue;
                        };
                        if write_controller_message(&mut stream, &hello).await.is_err() {
                            continue;
                        }
                        let Ok(Some(NodeToControllerMessage::HelloAccepted(_))) =
                            read_node_message(&mut stream).await
                        else {
                            continue;
                        };
                        while let Ok(Some(message)) = read_node_message(&mut stream).await {
                            if let NodeToControllerMessage::CloneResult(result) = message
                                && result.execution_id.as_str() == receipt.execution_id
                            {
                                return;
                            }
                        }
                    }
                })
                .await
                .unwrap();
                first.0.signal(ProcessSignal::Terminate).unwrap();
                until(|| first.0.has_exited().unwrap());
                // A replacement composition starts a fresh Node, which replays the original result.
                let (replacement, _) =
                    minicloud::launch(&fixture, &config, port, NodeHosting::Managed);
                controller = replacement;
                let second = hosted_node(&node_config);
                let record = tokio::time::timeout(Duration::from_secs(/*secs*/ 40), async {
                    loop {
                        let records: Vec<MiniCloneOperation> = client
                            .get(&clones)
                            .send()
                            .await
                            .unwrap()
                            .json()
                            .await
                            .unwrap();
                        assert_eq!(records.len(), 1);
                        if !matches!(records[0].state, MiniCloneState::Pending) {
                            break records[0].clone();
                        }
                        tokio::time::sleep(Duration::from_millis(/*millis*/ 50)).await;
                    }
                })
                .await
                .unwrap();
                let MiniCloneState::Succeeded { ref commit, .. } = record.state else {
                    panic!("{record:?}");
                };
                assert_eq!(commit, &expected_commit);
                assert_eq!(record.execution_id, receipt.execution_id);
                // Normal stop retires the hosted Node before the Controller exits.
                controller.terminate();
                assert!(controller.0.try_wait().unwrap().unwrap().success());
                assert!(second.0.has_exited().unwrap());
                ExecutionId::new(receipt.execution_id)
            });
        let database = ora_node_db::NodeDatabase::open(
            &fixture.config().home_directory.join("ora-node.sqlite3"),
            fixture.config().identity,
        )
        .unwrap();
        assert_eq!(
            database
                .process_journal()
                .unwrap()
                .attempts(&execution)
                .unwrap()
                .len(),
            1
        );
    });
}

/// Starts a hosting Controller that is expected to refuse composition; callers reap its exit status.
fn launch_expecting_exit(
    fixture: &Fixture,
    config: &DeploymentConfig,
) -> (crate::support::ChildGuard, std::path::PathBuf) {
    let path = fixture.path().join("refused-controller.json");
    fs::write(&path, serde_json::to_vec(config).unwrap()).unwrap();
    let child = crate::support::ChildGuard(
        std::process::Command::new(
            std::path::Path::new(env!("CARGO_BIN_EXE_ora-node")).with_file_name("ora-controller"),
        )
        .arg("--config")
        .arg(&path)
        .args(["--single-node", "--port", "0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap(),
    );
    (child, path)
}
