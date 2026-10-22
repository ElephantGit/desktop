//! Observes every host-to-plugin frame in a real local Deno fixture, without a provider or network.
use super::*;
use pretty_assertions::assert_eq;
use std::time::Duration;

const OBSERVER: &str = r#"
function send(value) {
  const payload = new TextEncoder().encode(JSON.stringify(value));
  const frame = new Uint8Array(payload.length + 5);
  new DataView(frame.buffer).setUint32(0, payload.length + 1);
  frame[4] = 1;
  frame.set(payload, 5);
  let offset = 0;
  while (offset < frame.length) offset += Deno.stdout.writeSync(frame.subarray(offset));
}
async function read(size) {
  const bytes = new Uint8Array(size);
  let offset = 0;
  while (offset < size) {
    const count = await Deno.stdin.read(bytes.subarray(offset));
    if (count === null) Deno.exit(0);
    offset += count;
  }
  return bytes;
}
const seen = [];
send({jsonrpc: "2.0", method: "ora/register", params: {
  methods: ["probe/control", "probe/snapshot", "agent/start", "agent/stop", "agent/list_models"],
  emits: ["agent/acp"]
}});
while (true) {
  const size = new DataView((await read(4)).buffer).getUint32(0);
  const frame = await read(size);
  if (frame[0] !== 1) throw new Error("unexpected transport tag");
  const message = JSON.parse(new TextDecoder().decode(frame.subarray(1)));
  if (message.method === "ora/shutdown") Deno.exit(0);
  if (message.method === "probe/snapshot") {
    send({jsonrpc: "2.0", id: message.id, result: seen});
    continue;
  }
  seen.push(message);
  if (message.method === "agent/acp") {
    const request = message.params;
    if (request.id !== undefined) send({jsonrpc: "2.0", method: "agent/acp", params: {
      jsonrpc: "2.0", id: request.id, result: request.method === "initialize"
        ? {protocolVersion: 1, agentCapabilities: {loadSession: true}}
        : request.method === "session/new" ? {sessionId: "unexpected-side-effect-session"}
        : request.method === "session/prompt" ? {stopReason: "end_turn"} : {}
    }});
    continue;
  }
  if (message.id !== undefined) send({jsonrpc: "2.0", id: message.id,
    result: message.method === "agent/list_models" ? {models: []}
      : message.method === "agent/start" ? {protocol: "acp", acpVersion: 1} : {}});
}
"#;

/// Positive control and ordered snapshot requests make the absence of plugin traffic observable.
#[test]
fn diagnostic_queries_do_not_start_stopped_plugins_or_call_a_running_generation() {
    run_test(async {
        let fixture = Fixture::stopped();
        // Isolate the cold query from discovery's ordinary background agent startup.
        fixture
            .sessions
            .agent_runtime
            .suspend_plugin_agent("official/ora-space.observer");
        let package = fixture
            ._temporary
            .path()
            .join("plugins")
            .join("installed")
            .join("official")
            .join("ora-space.observer")
            .join("1.0.0");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("main.js"), OBSERVER).unwrap();
        std::fs::write(package.join("orax.toml"), "resolver = 1\nidentifier = \"ora-space.observer\"\nnamespace = \"official\"\nkind = \"agent\"\nversion = \"1.0.0\"\ndescription = \"Diagnostic wire observer\"\n").unwrap();
        fixture.plugins.scan(ScanPluginsRequest {}).await.unwrap();
        let plugin_id = ora_domain::PluginId::parse("official/ora-space.observer").unwrap();
        let gateway = fixture.plugins.gateway();
        assert!(gateway.installed_plugin(&plugin_id).is_some());
        let agent = AgentRef::for_plugin(&plugin_id);
        fixture
            .sessions
            .repository
            .update_session_binding(
                &SessionId::new("session-1"),
                agent.clone(),
                "provider-session",
                2,
            )
            .unwrap();
        let mut initial = meta(&fixture, "provider-session");
        initial.agent_ref = agent.clone();
        let other = AgentRef::parse("missing/other").unwrap();
        let mut segments = history(
            &fixture,
            vec![
                HistoryRecord::Meta(initial),
                HistoryRecord::AgentSwitched(AgentSwitch {
                    from: agent.clone(),
                    to: other.clone(),
                    agent_session_id: "other-session".into(),
                }),
                HistoryRecord::AgentSwitched(AgentSwitch {
                    from: other,
                    to: agent.clone(),
                    agent_session_id: "provider-session".into(),
                }),
            ],
        );
        segments[2].current = true;
        assert_eq!(
            fixture
                .plugins
                .list_installed(ListInstalledPluginsRequest {})
                .unwrap()
                .plugins[0]
                .runtime,
            PluginRuntimeStatus::Stopped
        );
        let cold = fixture
            .sessions
            .bindings(GetSessionBindingsRequest {
                session_ids: vec!["session-1".into()],
            })
            .await
            .unwrap();
        assert_eq!(
            cold,
            GetSessionBindingsResponse {
                sessions: vec![SessionBindings::Available {
                    session_id: "session-1".into(),
                    history: BindingHistoryState::Readable,
                    segments
                }]
            }
        );
        assert_eq!(
            fixture
                .plugins
                .list_installed(ListInstalledPluginsRequest {})
                .unwrap()
                .plugins[0]
                .runtime,
            PluginRuntimeStatus::Stopped
        );

        // Resume normal supervision and wait for ACP readiness before taking the wire baseline.
        fixture
            .sessions
            .agent_runtime
            .resume_plugin_agent("official/ora-space.observer");
        fixture.sessions.agent_runtime.sync_plugin_agents();
        let runtime =
            crate::agent_runtime::AgentRuntime::new(fixture.sessions.agent_runtime.clone());
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if runtime
                    .status(GetAgentRuntimeStatusRequest {})
                    .unwrap()
                    .statuses
                    .iter()
                    .any(|entry| {
                        entry.agent_ref == "official/ora-space.observer"
                            && entry.status == AgentStatus::Ready
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("ordinary agent startup reached ACP readiness");
        let lease = gateway
            .ensure_running(&plugin_id, Duration::from_secs(15))
            .await
            .expect("real observer process registered");
        lease
            .invoke(
                "probe/control",
                serde_json::json!({"canary": "observer-is-live"}),
            )
            .await
            .unwrap();
        let before = lease
            .invoke("probe/snapshot", serde_json::json!({}))
            .await
            .unwrap();
        assert!(
            before
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["method"] == "probe/control"
                    && message["params"]["canary"] == "observer-is-live")
        );
        let root = fixture._temporary.path().join("sessions");
        let path = ora_history::history_path(&root, "session-1").unwrap();
        let mut observations = Vec::new();
        for mode in ["readable", "corrupt", "unavailable", "uncreated"] {
            match mode {
                "corrupt" => std::fs::write(&path, b"broken\n").unwrap(),
                "unavailable" => std::fs::remove_file(&path).unwrap(),
                "uncreated" => {
                    std::fs::write(&path, b"").unwrap();
                    fixture
                        .sessions
                        .repository
                        .update_session_binding(&SessionId::new("session-1"), agent.clone(), "", 3)
                        .unwrap();
                }
                _ => {}
            }
            let response = tokio::time::timeout(
                Duration::from_secs(10),
                fixture.sessions.bindings(GetSessionBindingsRequest {
                    session_ids: vec!["session-1".into(), "missing".into()],
                }),
            )
            .await
            .unwrap()
            .unwrap();
            observations.push(response);
        }
        let after = lease
            .invoke("probe/snapshot", serde_json::json!({}))
            .await
            .unwrap();
        let generation = gateway.connection(&plugin_id).unwrap().key();
        fixture
            .sessions
            .agent_runtime
            .suspend_plugin_agent("official/ora-space.observer");
        gateway.stop_if_idle(&plugin_id).await.unwrap();
        assert_eq!(
            before, after,
            "all frames, including ACP and lifecycle requests, stay unchanged"
        );
        assert_eq!(generation, lease.key());
        for (response, state) in observations.iter().zip([
            BindingHistoryState::Readable,
            BindingHistoryState::Incomplete,
            BindingHistoryState::Unavailable,
            BindingHistoryState::Readable,
        ]) {
            assert!(
                matches!(&response.sessions[0], SessionBindings::NotFound { session_id } if session_id == "missing")
            );
            assert!(
                matches!(&response.sessions[1], SessionBindings::Available { history, .. } if *history == state)
            );
        }
        assert!(
            matches!(&observations[3].sessions[1], SessionBindings::Available { segments, .. } if segments.is_empty())
        );
    });
}
