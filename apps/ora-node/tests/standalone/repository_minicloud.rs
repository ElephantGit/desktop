use super::*;
use crate::support::{ChildGuard, until};
use ora_contracts::minicloud::*;
use ora_controller::{NodeEndpoint, RuntimeConfig, SessionConfig};
use ora_minicloud_server::ServerConfig;
use pretty_assertions::assert_eq;
use std::{
    process::{Command, Stdio},
    time::Duration,
};

enum Entry {
    Http,
    Vite,
}

/// Starts a production server process and reads its actual ephemeral loopback address.
fn launch(fixture: &Fixture, config: &ServerConfig) -> (ChildGuard, String) {
    let path = fixture.path().join("minicloud.json");
    fs::write(&path, serde_json::to_vec(config).unwrap()).unwrap();
    let log = fixture.path().join("minicloud.log");
    let child = ChildGuard(
        Command::new(
            std::path::Path::new(env!("CARGO_BIN_EXE_ora-node"))
                .with_file_name("ora-minicloud-server"),
        )
        .arg(path)
        .stdin(Stdio::null())
        .stdout(fs::File::create(&log).unwrap())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("build ora-minicloud-server before standalone acceptance"),
    );
    let mut address = None;
    until(|| {
        address = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .find_map(|line| {
                line.strip_prefix("minicloud listening on ")
                    .map(str::to_owned)
            });
        address.is_some()
    });
    (child, address.unwrap())
}

/// Exercises actual proxy/HTTP, independent server death, Node and HTTPS Git without a fake coordinator.
fn exercise(entry: Entry) {
    ora_logging::with_trace_logging(|| {
        let fixture = Fixture::new();
        fixture.git(&["update-server-info"]);
        let expected_commit = fixture.git(&["rev-parse", "main"]);
        let source = HttpsRepository::new(fixture.path(), fixture.path().join("main").join(".git"));
        source.paused.store(true, Ordering::SeqCst);
        let clone = configuration(&fixture, &source);
        let mut node = ipc::launch(&fixture, &clone);
        until(|| {
            fs::read_to_string(fixture.path().join("ipc.log"))
                .unwrap_or_default()
                .contains("Node IPC listening")
        });
        let mut config = ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            node_id: NodeId::new("test-node"),
            controller: RuntimeConfig {
                home_directory: fixture.path().join("controller"),
                protected_state_directories: vec![
                    fixture.config().home_directory,
                    fixture.process().host_directory,
                ],
                controller_id: ControllerId::new("owner"),
                nodes: vec![NodeEndpoint {
                    node_id: NodeId::new("test-node"),
                    endpoint: fixture.config().home_directory.join("control.sock"),
                }],
                session: SessionConfig {
                    io_timeout_ms: 5000,
                    query_interval_ms: 100,
                },
                reconnect_ms: 100,
                timezone: "Asia/Shanghai".into(),
            },
        };
        let (mut server, address) = launch(&fixture, &config);
        config.listen = address.parse().unwrap();
        let mut vite = None;
        let base = match entry {
            Entry::Http => format!("http://{address}"),
            Entry::Vite => {
                let log = fixture.path().join("vite.log");
                vite = Some(ChildGuard(
                    Command::new(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../node_modules/.bin/vite"),
                    )
                    .current_dir(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../minicloud/client"),
                    )
                    .args(["--port", "0"])
                    .env("MINICLOUD_SERVER_URL", format!("http://{address}"))
                    .env("NO_COLOR", "1")
                    .stdout(fs::File::create(&log).unwrap())
                    .stderr(Stdio::inherit())
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap(),
                ));
                let mut url = None;
                until(|| {
                    url = fs::read_to_string(&log)
                        .unwrap_or_default()
                        .split_whitespace()
                        .find(|part| part.starts_with("http://127.0.0.1:"))
                        .map(|part| part.trim_end_matches('/').to_owned());
                    url.is_some()
                });
                url.unwrap()
            }
        };
        let execution = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let client = reqwest::Client::builder()
                    .pool_max_idle_per_host(/*max*/ 0)
                    .build()
                    .unwrap();
                if vite.is_some() {
                    let page = client
                        .get(&base)
                        .send()
                        .await
                        .unwrap()
                        .text()
                        .await
                        .unwrap();
                    assert!(page.contains("/src/main.tsx"));
                }
                let endpoint = format!("{base}/api/clones");
                let input = MiniCloneRequest {
                    request_id: "web-request".into(),
                    repository: source.address.clone(),
                    branch: "main".into(),
                };
                // Hold a real SQLite writer lock at the persistence seam. HTTP must not report
                // acceptance or leave dispatchable intent when its durable write cannot commit.
                let locked = rusqlite::Connection::open(
                    config
                        .controller
                        .home_directory
                        .join("ora-controller.sqlite3"),
                )
                .unwrap();
                locked.execute_batch("BEGIN IMMEDIATE").unwrap();
                let rejected = client.post(&endpoint).json(&input).send().await.unwrap();
                assert_eq!(rejected.status().as_u16(), 503);
                locked.execute_batch("ROLLBACK").unwrap();
                drop(locked);
                assert_eq!(
                    client
                        .get(&endpoint)
                        .send()
                        .await
                        .unwrap()
                        .json::<Vec<MiniCloneOperation>>()
                        .await
                        .unwrap(),
                    vec![]
                );
                let response = client.post(&endpoint).json(&input).send().await.unwrap();
                assert_eq!(response.status().as_u16(), 202);
                let receipt: MiniCloneAccepted = response.json().await.unwrap();
                server.kill();
                let (replacement, _) = launch(&fixture, &config);
                server = replacement;
                assert_eq!(
                    client
                        .post(&endpoint)
                        .json(&input)
                        .send()
                        .await
                        .unwrap()
                        .json::<MiniCloneAccepted>()
                        .await
                        .unwrap(),
                    receipt
                );
                source.paused.store(false, Ordering::SeqCst);
                let final_record = tokio::time::timeout(Duration::from_secs(/*secs*/ 40), async {
                    loop {
                        let records: Vec<MiniCloneOperation> = client
                            .get(&endpoint)
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
                let MiniCloneState::Succeeded {
                    ref path,
                    ref commit,
                } = final_record.state
                else {
                    panic!("{final_record:?}");
                };
                assert_eq!(commit, &expected_commit);
                assert!(std::path::Path::new(path).join(".git").is_dir());
                source.reject_auth.store(true, Ordering::SeqCst);
                server.kill();
                let (replacement, _) = launch(&fixture, &config);
                server = replacement;
                let restored: MiniCloneOperation = client
                    .get(format!("{endpoint}/{}", receipt.execution_id))
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                assert_eq!(restored, final_record);
                assert_eq!(receipt.execution_id, restored.execution_id);
                ExecutionId::new(receipt.execution_id)
            });
        // Vite handles normal termination; reap it before deleting its fixture output directory.
        if let Some(mut vite) = vite {
            vite.terminate();
        }
        server.terminate();
        node.terminate();
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

/// Ordinary crates acceptance needs no JavaScript installation and exercises the independent HTTP executable.
#[test]
fn minicloud_http_clone_survives_server_kill_and_replays_original_intent() {
    exercise(Entry::Http);
}

/// Explicit development acceptance adds real Vite proxy; run after installing frontend dependencies.
#[test]
#[ignore = "requires deno install; run the minicloud_vite filter explicitly with --ignored"]
fn minicloud_vite_proxy_reaches_real_https_clone_and_restart() {
    exercise(Entry::Vite);
}
