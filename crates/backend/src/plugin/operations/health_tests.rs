//! Behavior tests for Host MCP health at the plugin-operation surface.
//!
//! These drive the real `Plugins` composition — install, save, uninstall, and the health queries —
//! so the evidence covers the paths a user actually takes, not the store in isolation.

use super::Plugins;
use crate::agent_runtime::{AgentRuntimeManager, AgentRuntimeSetup};
use crate::app_event::AppEventHub;
use crate::clock::SystemClock;
use crate::plugin::PluginApi;
use crate::settings::Settings;
use ora_contracts::{
    AppEvent, GetPluginConfigurationRequest, ImportPluginRequest, ListMcpHealthRequest,
    ListMcpHealthResponse, McpHealthErrorCode, McpHealthStatus, McpHealthUnknownReason,
    PluginConfigurationCompleteness, PluginSettingValue, ProbeMcpHealthRequest,
    SavePluginConfigurationRequest, UninstallPluginRequest,
};
use ora_db::{DatabaseBootstrapper, DatabaseLocation, RepositoryPool, default_migration_catalog};
use ora_scheduler::Scheduler;
use pretty_assertions::assert_eq;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Opens a throwaway SQLite pool under `root`.
fn test_pool(root: &Path) -> RepositoryPool {
    ora_logging::initialize_test_clock();
    DatabaseBootstrapper::new(crate::test_clock::TestClock)
        .bootstrap_repository_pool(
            &DatabaseLocation::path(root.join("test.sqlite")),
            &default_migration_catalog().expect("build migration catalog"),
        )
        .expect("create repository pool")
}

/// Exercises the public plugin interface together with the event hub it publishes into.
fn test_plugins(root: &Path, pool: &RepositoryPool) -> (Plugins, AppEventHub) {
    let events = AppEventHub::new();
    let host = Arc::new(
        PluginApi::open(
            pool.clone(),
            root.to_path_buf(),
            std::path::PathBuf::from("deno"),
            SystemClock,
            events.publisher(),
            Arc::new(Settings::new(pool.clone())),
        )
        .expect("open plugin host"),
    );
    let runtime = Arc::new(
        AgentRuntimeManager::new(AgentRuntimeSetup {
            plugin_host: host.clone(),
            pool: pool.clone(),
            home_directory: root.to_path_buf(),
            relative_path_base: root.to_path_buf(),
            sessions_root: root.join("sessions"),
            clock: SystemClock,
            scheduler: Scheduler::new(chrono_tz::Asia::Shanghai),
            app_events: events.publisher(),
        })
        .expect("agent runtime"),
    );
    (Plugins::new(host, runtime), events)
}

/// Writes an MCP `.orax` archive with the given `assets/config.json` body.
///
/// The stdio command is a regular file inside the package that no platform can execute — a missing
/// interpreter for Unix and an unusable image for Windows — so both reach `mcp_spawn_failed`
/// without depending on the host's installed programs.
fn write_mcp_orax(path: &Path, identifier: &str, config: &str) {
    let manifest = format!(
        "resolver = 1\nidentifier = \"{identifier}\"\nnamespace = \"official\"\nkind = \"mcp\"\nversion = \"0.1.0\"\ndescription = \"MCP probe fixture\"\n"
    );
    let mut writer = ZipWriter::new(File::create(path).expect("create archive"));
    let options = SimpleFileOptions::default();
    writer.start_file("orax.toml", options).expect("manifest");
    writer
        .write_all(manifest.as_bytes())
        .expect("write manifest");
    writer
        .start_file("assets/config.json", options)
        .expect("config");
    writer.write_all(config.as_bytes()).expect("write config");
    writer
        .start_file("assets/server", options)
        .expect("command file");
    writer
        .write_all(b"#!/nonexistent-ora-probe-interpreter\n")
        .expect("write command");
    writer.finish().expect("finish archive");
}

/// Imports one MCP archive and returns its canonical plugin id.
async fn import_mcp(plugins: &Plugins, root: &Path, identifier: &str, config: &str) -> String {
    let archive = root.join(format!("{identifier}.orax"));
    write_mcp_orax(&archive, identifier, config);
    let response = plugins
        .import(ImportPluginRequest {
            path: archive.to_string_lossy().into_owned(),
        })
        .await
        .expect("import MCP package");
    make_command_executable(root, identifier);
    response.plugin_id
}

/// Grants the extracted command an execute bit where the platform requires one.
fn make_command_executable(root: &Path, identifier: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let command = root
            .join("plugins/installed/official")
            .join(identifier)
            .join("0.1.0/assets/server");
        if command.is_file() {
            std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755))
                .expect("make command executable");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (root, identifier);
    }
}

/// Polls the card health view until the named plugin leaves `Unknown(not_probed)`.
async fn wait_for_card_health(plugins: &Plugins, plugin_id: &str) -> ListMcpHealthResponse {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let listed = plugins
            .list_mcp_health(ListMcpHealthRequest { cwd: None })
            .expect("list mcp health");
        let settled = listed.entries.iter().find(|entry| {
            entry.identity.plugin_id == plugin_id
                && !matches!(
                    entry.status,
                    McpHealthStatus::Unknown {
                        reason: McpHealthUnknownReason::NotProbed
                    }
                )
        });
        if settled.is_some() {
            return listed;
        }
        if Instant::now() >= deadline {
            panic!("health for {plugin_id} did not settle: {listed:?}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The default fixture: a stdio MCP with no Settings and an unrunnable command.
const UNRUNNABLE_STDIO_CONFIG: &str = r#"{
    "schemaVersion": 1,
    "transport": {
        "type": "stdio",
        "command": "assets/server"
    }
}"#;

/// A stdio MCP whose argument substitutes the Session workspace directory.
const WORKSPACE_CONTEXT_STDIO_CONFIG: &str = r#"{
    "schemaVersion": 1,
    "transport": {
        "type": "stdio",
        "command": "assets/server",
        "args": [{ "context": "workspace" }]
    }
}"#;

/// A stdio MCP that binds a required Setting into the process environment.
const SECRET_ENV_STDIO_CONFIG: &str = r#"{
    "schemaVersion": 1,
    "settings": {
        "apiKey": {
            "type": "string",
            "title": "API key",
            "description": "Credential passed to the server process",
            "required": true
        }
    },
    "transport": {
        "type": "stdio",
        "command": "assets/server",
        "env": {
            "ORA_FIXTURE_TOKEN": { "setting": "apiKey", "prefix": "Bearer " }
        }
    }
}"#;

/// Installing an unrunnable MCP publishes exactly one closed code on the card.
#[test]
fn installing_an_unrunnable_mcp_publishes_spawn_failed() {
    ora_logging::with_trace_logging(|| {
        let temporary = TempDir::new().expect("temp directory");
        let pool = test_pool(temporary.path());
        let (plugins, hub) = test_plugins(temporary.path(), &pool);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                // Subscribing needs the runtime: it starts the forwarding task.
                let mut events = hub.subscribe();
                assert_eq!(
                    events.recv().await.expect("ready").expect("event"),
                    AppEvent::Ready
                );
                let plugin_id = import_mcp(
                    &plugins,
                    temporary.path(),
                    "unrunnable-mcp",
                    UNRUNNABLE_STDIO_CONFIG,
                )
                .await;
                let listed = wait_for_card_health(&plugins, &plugin_id).await;
                assert_eq!(
                    listed.entries,
                    vec![ora_contracts::McpHealthEntry {
                        identity: ora_contracts::McpHealthIdentity {
                            plugin_id: plugin_id.clone(),
                            package_version: "0.1.0".to_string(),
                            configuration_revision: 0,
                            transport: ora_contracts::McpHealthTransport::Stdio,
                            cwd: None,
                        },
                        status: McpHealthStatus::Unhealthy {
                            error_code: McpHealthErrorCode::McpSpawnFailed
                        },
                    }]
                );
                // The card is told to re-query; the event carries identity only.
                let mut saw_changed = false;
                for _ in 0..64 {
                    match tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
                        Ok(Some(Ok(event))) => {
                            if event
                                == (AppEvent::McpHealthChanged {
                                    plugin_id: plugin_id.clone(),
                                })
                            {
                                saw_changed = true;
                            }
                        }
                        Ok(Some(Err(_))) | Ok(None) | Err(_) => break,
                    }
                }
                assert!(saw_changed, "health change must reach the card");
            });
    });
}

/// A workspace-context MCP has no card identity and must never be probed against a fake cwd.
#[test]
fn workspace_context_mcp_stays_context_missing_on_the_card() {
    ora_logging::with_trace_logging(|| {
        let temporary = TempDir::new().expect("temp directory");
        let pool = test_pool(temporary.path());
        let (plugins, _hub) = test_plugins(temporary.path(), &pool);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let plugin_id = import_mcp(
                    &plugins,
                    temporary.path(),
                    "workspace-mcp",
                    WORKSPACE_CONTEXT_STDIO_CONFIG,
                )
                .await;
                // Give any erroneous probe a chance to run before asserting it never did.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let listed = plugins
                    .list_mcp_health(ListMcpHealthRequest { cwd: None })
                    .expect("list mcp health");
                assert_eq!(
                    listed.entries[0].status,
                    McpHealthStatus::Unknown {
                        reason: McpHealthUnknownReason::ContextMissing
                    }
                );
                // Re-detect from the card must not invent a cwd either.
                let probed = plugins
                    .probe_mcp_health(ProbeMcpHealthRequest {
                        plugin_id: plugin_id.clone(),
                        cwd: None,
                    })
                    .await
                    .expect("card re-detect");
                assert_eq!(
                    probed.entry.status,
                    McpHealthStatus::Unknown {
                        reason: McpHealthUnknownReason::ContextMissing
                    }
                );
            });
    });
}

/// Required Settings keep a member out of probing until a complete save qualifies it.
#[test]
fn incomplete_settings_are_not_probed_until_a_complete_save() {
    ora_logging::with_trace_logging(|| {
        let temporary = TempDir::new().expect("temp directory");
        let pool = test_pool(temporary.path());
        let (plugins, _hub) = test_plugins(temporary.path(), &pool);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let plugin_id = import_mcp(
                    &plugins,
                    temporary.path(),
                    "configured-mcp",
                    SECRET_ENV_STDIO_CONFIG,
                )
                .await;

                // Incomplete: no probe is started and the card shows no health row at all.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let before = plugins
                    .list_mcp_health(ListMcpHealthRequest { cwd: None })
                    .expect("list before saving");
                assert!(before.entries.is_empty(), "{before:?}");

                let details = plugins
                    .get_configuration(GetPluginConfigurationRequest {
                        plugin_id: plugin_id.clone(),
                    })
                    .expect("configuration")
                    .configuration;
                let saved = plugins
                    .save_configuration(SavePluginConfigurationRequest {
                        plugin_id: plugin_id.clone(),
                        expected_revision: details.revision,
                        declaration_fingerprint: details.declaration_fingerprint.clone(),
                        values: std::collections::BTreeMap::from([(
                            "apiKey".to_string(),
                            PluginSettingValue::String("super-secret-key".into()),
                        )]),
                        preserve_setting_ids: Vec::new(),
                    })
                    .expect("save configuration")
                    .configuration;
                assert_eq!(
                    saved.summary,
                    ora_contracts::PluginConfigurationSummary::Available {
                        completeness: PluginConfigurationCompleteness::Complete
                    }
                );
                assert_ne!(saved.revision, details.revision);

                // The save itself probed once and only once, on the new revision.
                let listed = wait_for_card_health(&plugins, &plugin_id).await;
                assert_eq!(listed.entries.len(), 1);
                assert_eq!(
                    listed.entries[0].identity.configuration_revision,
                    saved.revision
                );
                assert_eq!(
                    listed.entries[0].status,
                    McpHealthStatus::Unhealthy {
                        error_code: McpHealthErrorCode::McpSpawnFailed
                    }
                );
            });
    });
}

/// Uninstalling a plugin removes its health rows everywhere.
#[test]
fn uninstall_removes_the_health_row() {
    ora_logging::with_trace_logging(|| {
        let temporary = TempDir::new().expect("temp directory");
        let pool = test_pool(temporary.path());
        let (plugins, _hub) = test_plugins(temporary.path(), &pool);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let plugin_id = import_mcp(
                    &plugins,
                    temporary.path(),
                    "unrunnable-mcp",
                    UNRUNNABLE_STDIO_CONFIG,
                )
                .await;
                wait_for_card_health(&plugins, &plugin_id).await;

                plugins
                    .uninstall(UninstallPluginRequest {
                        plugin_id: plugin_id.clone(),
                        data_disposition: ora_contracts::PluginDataDisposition::Delete,
                    })
                    .await
                    .expect("uninstall");

                let listed = plugins
                    .list_mcp_health(ListMcpHealthRequest { cwd: None })
                    .expect("list after uninstall");
                assert!(listed.entries.is_empty(), "{listed:?}");
            });
    });
}

/// No health surface may carry a Setting value, credential, or process environment.
#[test]
fn health_surfaces_never_expose_setting_values() {
    let temporary = TempDir::new().expect("temp directory");
    let pool = test_pool(temporary.path());
    let (plugins, _hub) = test_plugins(temporary.path(), &pool);
    let recorder = EventTextRecorder::default();
    ora_logging::with_recorded_trace_logging(recorder.layer(), || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let plugin_id = import_mcp(
                    &plugins,
                    temporary.path(),
                    "secret-mcp",
                    SECRET_ENV_STDIO_CONFIG,
                )
                .await;
                let details = plugins
                    .get_configuration(GetPluginConfigurationRequest {
                        plugin_id: plugin_id.clone(),
                    })
                    .expect("configuration")
                    .configuration;
                plugins
                    .save_configuration(SavePluginConfigurationRequest {
                        plugin_id: plugin_id.clone(),
                        expected_revision: details.revision,
                        declaration_fingerprint: details.declaration_fingerprint,
                        values: std::collections::BTreeMap::from([(
                            "apiKey".to_string(),
                            PluginSettingValue::String("super-secret-key".into()),
                        )]),
                        preserve_setting_ids: Vec::new(),
                    })
                    .expect("save configuration");
                let listed = wait_for_card_health(&plugins, &plugin_id).await;
                let probed = plugins
                    .probe_mcp_health(ProbeMcpHealthRequest {
                        plugin_id: plugin_id.clone(),
                        cwd: None,
                    })
                    .await
                    .expect("re-detect");

                // The installed-plugin listing, the query, and the probe response are all
                // serialized exactly as the frontend would receive them.
                let installed = serde_json::to_string(
                    &plugins
                        .list_installed(ora_contracts::ListInstalledPluginsRequest {})
                        .expect("list installed"),
                )
                .expect("serialize installed");
                let listed_json = serde_json::to_string(&listed).expect("serialize list");
                let probed_json = serde_json::to_string(&probed).expect("serialize probe");
                for surface in [&installed, &listed_json, &probed_json] {
                    assert!(!surface.contains("super-secret-key"), "{surface}");
                    assert!(!surface.contains("ORA_FIXTURE_TOKEN"), "{surface}");
                    assert!(!surface.contains("Bearer"), "{surface}");
                }
                assert_eq!(listed.entries.len(), 1);
                assert_eq!(
                    listed.entries[0].status,
                    McpHealthStatus::Unhealthy {
                        error_code: McpHealthErrorCode::McpSpawnFailed
                    }
                );
            });
    });

    let recorded = recorder.text();
    assert!(!recorded.contains("super-secret-key"), "{recorded}");
    assert!(!recorded.contains("ORA_FIXTURE_TOKEN"), "{recorded}");
    assert!(!recorded.contains("Bearer"), "{recorded}");
}

/// Captures the rendered fields of every event emitted into the scoped TRACE subscriber.
#[derive(Clone, Debug, Default)]
struct EventTextRecorder {
    text: Arc<std::sync::Mutex<String>>,
}

impl EventTextRecorder {
    fn layer(&self) -> EventTextLayer {
        EventTextLayer {
            text: self.text.clone(),
        }
    }

    fn text(&self) -> String {
        self.text.lock().expect("recorded event lock").clone()
    }
}

#[derive(Clone, Debug)]
struct EventTextLayer {
    text: Arc<std::sync::Mutex<String>>,
}

impl<S> tracing_subscriber::layer::Layer<S> for EventTextLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        event.record(&mut EventTextVisitor {
            text: self.text.clone(),
        });
    }
}

/// Renders each field as `name=value` so the test can assert the whole leak boundary.
struct EventTextVisitor {
    text: Arc<std::sync::Mutex<String>>,
}

impl EventTextVisitor {
    fn record(&mut self, field: &tracing::field::Field, value: impl std::fmt::Display) {
        let mut text = self.text.lock().expect("recorded event lock");
        text.push_str(field.name());
        text.push('=');
        text.push_str(&value.to_string());
        text.push('\n');
    }
}

impl tracing::field::Visit for EventTextVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.record(field, value);
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record(field, format!("{value:?}"));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.record(field, value);
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.record(field, value);
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.record(field, value);
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.record(field, value);
    }
}
