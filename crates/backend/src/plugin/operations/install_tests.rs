//! Covers Hook install outcomes after the host dropped plugin enablement.

use super::Plugins;
use crate::agent_runtime::{AgentRuntimeManager, AgentRuntimeSetup};
use crate::app_event::AppEventHub;
use crate::clock::SystemClock;
use crate::plugin::PluginApi;
use crate::settings::Settings;
use ora_contracts::{
    ImportPluginRequest, ImportedWorkflowOutcome, InstallOutcome, ListInstalledPluginsRequest,
};
use ora_db::{DatabaseBootstrapper, DatabaseLocation, RepositoryPool, default_migration_catalog};
use ora_logging::with_trace_logging;
use ora_scheduler::Scheduler;
use pretty_assertions::assert_eq;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Opens a throwaway SQLite pool under `root` for PluginApi tests.
fn test_pool(root: &Path) -> RepositoryPool {
    ora_logging::initialize_test_clock();
    DatabaseBootstrapper::new(crate::test_clock::TestClock)
        .bootstrap_repository_pool(
            &DatabaseLocation::path(root.join("test.sqlite")),
            &default_migration_catalog().expect("build migration catalog"),
        )
        .expect("create repository pool")
}

/// Exercises the public plugin interface with its real host and shared runtime coordinator.
fn test_plugin_api(root: &Path, pool: &RepositoryPool) -> Plugins {
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
    Plugins::new(
        host,
        runtime,
        Arc::new(crate::workflow::workflow_import(pool.clone(), SystemClock)),
    )
}

/// Writes a processless Hook `.orax` whose command alias is `rtk` and whose artifact matches
/// `host`.
fn write_hook_orax(path: &Path, identifier: &str, host: &str) {
    let manifest = format!(
        "resolver = 1\nidentifier = \"{identifier}\"\nnamespace = \"official\"\nkind = \"hook\"\nversion = \"0.1.0\"\ndescription = \"Hook command rewrite\"\n\n[artifact]\ntarget = \"{host}\"\n"
    );
    let config = br#"{"schemaVersion":1,"hook":{"protocol":"rtk-rewrite-v1","executable":"assets/rtk.exe","command":"rtk","toolVersion":"0.45.0"}}"#;
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    let options = SimpleFileOptions::default();
    writer.start_file("orax.toml", options).unwrap();
    writer.write_all(manifest.as_bytes()).unwrap();
    writer.start_file("assets/config.json", options).unwrap();
    writer.write_all(config).unwrap();
    writer.start_file("assets/rtk.exe", options).unwrap();
    writer.write_all(b"MZdummy").unwrap();
    writer.finish().unwrap();
}

/// Marketplace README reads resolve from the source checkout beside the listing's manifest.
#[test]
fn read_plugin_readme_resolves_from_the_marketplace_checkout() {
    with_trace_logging(|| {
        let data_dir = TempDir::new().expect("data dir");
        let pool = test_pool(data_dir.path());
        let api = test_plugin_api(data_dir.path(), &pool);
        let checkout = data_dir
            .path()
            .join("plugins")
            .join("sources")
            .join("github.com")
            .join("ora-space")
            .join("marketplace");
        let digest = "ab".repeat(32);

        let listing_dir = checkout
            .join("registry")
            .join("o")
            .join("ora-space.weather");
        std::fs::create_dir_all(&listing_dir).expect("create listing dir");
        std::fs::write(
            listing_dir.join("orax.toml"),
            format!(
                "resolver = 1\nidentifier = \"ora-space.weather\"\nnamespace = \"official\"\nkind = \"agent\"\nversion = \"1.2.0\"\ndescription = \"Weather plugin\"\nurl = \"https://example.com/weather.orax\"\nsha256 = \"{digest}\"\n"
            ),
        )
        .expect("write listing manifest");
        std::fs::write(
            listing_dir.join("README.md"),
            "# Weather\n\nLive forecasts.",
        )
        .expect("write listing README");

        let response = api
            .read_readme(ora_contracts::ReadPluginReadmeRequest {
                plugin_id: "official/ora-space.weather".to_string(),
            })
            .expect("read readme");
        assert_eq!(
            response.readme.as_deref(),
            Some("# Weather\n\nLive forecasts.")
        );

        // A listing without a README reports no documentation; an unknown id reports NotFound.
        let silent_dir = checkout.join("registry").join("s").join("ora-space.silent");
        std::fs::create_dir_all(&silent_dir).expect("create silent listing dir");
        std::fs::write(
            silent_dir.join("orax.toml"),
            format!(
                "resolver = 1\nidentifier = \"ora-space.silent\"\nnamespace = \"official\"\nkind = \"agent\"\nversion = \"1.0.0\"\ndescription = \"Silent plugin\"\nurl = \"https://example.com/silent.orax\"\nsha256 = \"{digest}\"\n"
            ),
        )
        .expect("write silent manifest");

        let silent = api
            .read_readme(ora_contracts::ReadPluginReadmeRequest {
                plugin_id: "official/ora-space.silent".to_string(),
            })
            .expect("read silent readme");
        assert_eq!(silent.readme, None);

        let unknown = api
            .read_readme(ora_contracts::ReadPluginReadmeRequest {
                plugin_id: "official/absent".to_string(),
            })
            .expect_err("unknown id");
        assert_eq!(
            unknown.to_string(),
            "marketplace plugin was not found in the registry"
        );
    });
}

/// Two Hook packages that share a command alias both stay installed; the second import reports
/// the colliding identity instead of claiming the new package was disabled.
#[test]
fn importing_a_second_hook_with_the_same_command_reports_a_conflict_without_disabling() {
    with_trace_logging(|| {
        let Some(host) = ora_plugin_registry::current_host_target() else {
            eprintln!(
                "skipping Hook command-conflict import: compiled host is not a plugin target"
            );
            return;
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async move {
                let data_dir = TempDir::new().expect("data dir");
                let pool = test_pool(data_dir.path());
                let api = test_plugin_api(data_dir.path(), &pool);
                let first = data_dir.path().join("first.orax");
                let second = data_dir.path().join("second.orax");
                write_hook_orax(&first, "rtk-ai.rtk", host.as_str());
                write_hook_orax(&second, "other.rtk", host.as_str());

                let first_response = api
                    .import(ImportPluginRequest {
                        path: first.to_string_lossy().into_owned(),
                    })
                    .await
                    .expect("import first Hook");
                assert_eq!(
                    first_response.outcome,
                    InstallOutcome::Installed,
                    "the first Hook must be available without a conflict"
                );

                let second_response = api
                    .import(ImportPluginRequest {
                        path: second.to_string_lossy().into_owned(),
                    })
                    .await
                    .expect("import second Hook");
                assert_eq!(
                    second_response.outcome,
                    InstallOutcome::InstalledWithCommandConflict {
                        conflict_plugin_id: "local/rtk-ai.rtk".to_string(),
                    }
                );

                let listed = api
                    .list_installed(ListInstalledPluginsRequest {})
                    .expect("installed snapshot");
                let ids: Vec<&str> = listed
                    .plugins
                    .iter()
                    .map(|plugin| plugin.id.as_str())
                    .collect();
                assert!(
                    ids.contains(&"local/rtk-ai.rtk") && ids.contains(&"local/other.rtk"),
                    "both Hooks must remain installed and available, got {ids:?}"
                );
            });
    });
}

/// One Start-only workflow document the run engine accepts, carrying an explicit version.
const WORKFLOW_DOCUMENT: &str = r#"{"name":"导入流程","version":"1.0.0","viewport":{"x":0,"y":0,"zoom":1},"nodes":[{"id":"start","type":"workflow","position":{"x":0,"y":0},"data":{"kind":"start","title":"开始"}}],"edges":[]}"#;

/// Writes a Workflow `.orax` carrying the given `assets/workflows/<name>` documents.
fn write_workflow_orax(path: &Path, identifier: &str, documents: &[(&str, &str)]) {
    let manifest = format!(
        "resolver = 1\nidentifier = \"{identifier}\"\nkind = \"workflow\"\nversion = \"0.1.0\"\ndescription = \"Workflow package\"\n"
    );
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    let options = SimpleFileOptions::default();
    writer.start_file("orax.toml", options).unwrap();
    writer.write_all(manifest.as_bytes()).unwrap();
    for (name, contents) in documents {
        writer
            .start_file(format!("assets/workflows/{name}"), options)
            .unwrap();
        writer.write_all(contents.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
}

/// A Workflow package installs and turns each document into a workflow with a published
/// snapshot, reporting one malformed document on its own without costing the user the working
/// workflow beside it.
#[test]
fn imports_workflow_package_documents_alongside_the_plugin() {
    with_trace_logging(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async move {
                let data_dir = TempDir::new().expect("data dir");
                let pool = test_pool(data_dir.path());
                let api = test_plugin_api(data_dir.path(), &pool);
                let archive = data_dir.path().join("workflows.orax");
                write_workflow_orax(
                    &archive,
                    "ora.workflows",
                    &[
                        ("1.0.0.json", WORKFLOW_DOCUMENT),
                        ("2.0.0.json", "{ not json"),
                    ],
                );

                let response = api
                    .import(ImportPluginRequest {
                        path: archive.to_string_lossy().into_owned(),
                    })
                    .await
                    .expect("import workflow package");

                // The package itself installs under the reserved local namespace.
                assert_eq!(response.plugin_id, "local/ora.workflows");
                assert_eq!(response.outcome, InstallOutcome::Installed);

                let [imported, failed] = response.workflows.as_slice() else {
                    panic!(
                        "expected two document outcomes, got {:?}",
                        response.workflows
                    );
                };
                let ImportedWorkflowOutcome::Imported {
                    source_file,
                    workflow_id,
                    name,
                    version,
                } = imported
                else {
                    panic!("expected the valid document to import, got {imported:?}");
                };
                assert_eq!(source_file, "assets/workflows/1.0.0.json");
                assert_eq!(name, "导入流程");
                assert_eq!(version, "1.0.0");
                assert!(
                    !workflow_id.is_empty(),
                    "import must report the created workflow"
                );

                let ImportedWorkflowOutcome::Failed {
                    source_file,
                    reason,
                } = failed
                else {
                    panic!("expected the malformed document to fail, got {failed:?}");
                };
                assert_eq!(source_file, "assets/workflows/2.0.0.json");
                assert!(reason.contains("not valid JSON"), "{reason}");
            });
    });
}
