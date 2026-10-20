#![allow(clippy::unwrap_used)]

use super::*;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

pub(crate) use super::test_gateway::Gateway;

#[tokio::test]
async fn renewable_access_keeps_token_in_memory_and_revokes_at_end() {
    let root = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(root.path());
    let mut access = ModelAccess::open(
        gateway.config.clone(),
        &ModelBindingId::new("binding-1"),
        &ExecutionId::new("execution-1"),
        root.path(),
        crate::model_proxy::ModelRoute::LegacyOpenCode,
    )
    .await
    .unwrap();
    let home = PathBuf::from(&access.environment["HOME"]);
    assert_eq!(
        access
            .environment
            .get("OPENCODE_DISABLE_PROJECT_CONFIG")
            .map(String::as_str),
        Some("true")
    );
    let config: Value =
        serde_json::from_str(&access.environment["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(config["model"], "ora-model/team/model");
    assert_eq!(
        config["provider"]["ora-model"]["options"]["apiKey"],
        "{env:ORA_MODEL_ACCESS_TOKEN}"
    );
    assert!(!config.to_string().contains("synthetic-temporary-token"));
    tokio::time::timeout(Duration::from_secs(/*secs*/ 4), async {
        while gateway.requests.lock().unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        access.environment["ORA_MODEL_ACCESS_TOKEN"],
        "synthetic-temporary-token"
    );
    access.close().await;
    drop(access);
    assert!(!home.exists());
    assert_eq!(
        *gateway.requests.lock().unwrap(),
        vec![
            (
                "POST /internal/v1/model-grants HTTP/1.1".to_string(),
                json!({"bindingId":"binding-1","executionId":"execution-1"})
            ),
            (
                "POST /internal/v1/model-grants/grant-1/renew HTTP/1.1".to_string(),
                json!({"executionId":"execution-1"})
            ),
            (
                "DELETE /internal/v1/model-grants/grant-1 HTTP/1.1".to_string(),
                json!({"executionId":"execution-1"})
            ),
        ]
    );
}

#[tokio::test]
async fn foreign_data_endpoint_is_rejected_and_grant_revoked() {
    let root = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(root.path());
    gateway
        .behavior
        .foreign_endpoint
        .store(true, Ordering::SeqCst);
    let result = ModelAccess::open(
        gateway.config.clone(),
        &ModelBindingId::new("binding-1"),
        &ExecutionId::new("execution-1"),
        root.path(),
        crate::model_proxy::ModelRoute::LegacyOpenCode,
    )
    .await;
    let Err(error) = result else {
        panic!("foreign endpoint must not receive a token")
    };
    assert_eq!(error.to_string(), "model_grant_invalid");
    assert_eq!(
        gateway
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(route, _)| route.clone())
            .collect::<Vec<_>>(),
        vec![
            "POST /internal/v1/model-grants HTTP/1.1",
            "DELETE /internal/v1/model-grants/grant-1 HTTP/1.1"
        ]
    );
}

#[tokio::test]
async fn denied_renewal_notifies_the_conversation_to_stop() {
    let root = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(root.path());
    gateway.behavior.deny_renewal.store(true, Ordering::SeqCst);
    let mut access = ModelAccess::open(
        gateway.config.clone(),
        &ModelBindingId::new("binding-1"),
        &ExecutionId::new("execution-1"),
        root.path(),
        crate::model_proxy::ModelRoute::LegacyOpenCode,
    )
    .await
    .unwrap();
    let status = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 4),
        access
            .status
            .wait_for(|status| *status != ModelAccessStatus::Active),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(*status, ModelAccessStatus::Revoked);
    drop(status);
    access.close().await;
    assert_eq!(gateway.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn close_cancels_the_scheduled_renewal() {
    let root = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(root.path());
    let mut access = ModelAccess::open(
        gateway.config.clone(),
        &ModelBindingId::new("binding-1"),
        &ExecutionId::new("execution-1"),
        root.path(),
        crate::model_proxy::ModelRoute::LegacyOpenCode,
    )
    .await
    .unwrap();
    access.close().await;
    // This crosses the fixture grant's expiry, so an unowned timer would have renewed by now.
    tokio::time::sleep(Duration::from_millis(/*millis*/ 2200)).await;
    assert_eq!(gateway.requests.lock().unwrap().len(), 2);
}

#[test]
fn anthropic_configuration_preserves_model_ids_and_isolates_cli_storage() {
    let root = tempfile::tempdir().unwrap();
    let grant = Grant {
        grant_id: "grant-1".into(),
        token: "synthetic-token".into(),
        expires_at: chrono::Local::now().fixed_offset(),
        protocol: ora_node_protocol::ModelProtocol::AnthropicMessages,
        proxy_base_url: "https://gateway/runtime/anthropic/v1".into(),
        model: Model {
            id: "team/model".into(),
            name: "Model".into(),
            context_window: 200000,
            max_tokens: 8192,
        },
    };
    let (environment, home) =
        opencode::environment(&grant, &root.path().join("ca.pem"), root.path()).unwrap();
    let config: Value = serde_json::from_str(&environment["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(config["provider"]["ora-model"]["npm"], "@ai-sdk/anthropic");
    assert_eq!(
        config["provider"]["ora-model"]["models"]["team/model"],
        json!({"name":"Model","limit":{"context":200000,"output":8192}})
    );
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    ] {
        assert!(PathBuf::from(&environment[key]).starts_with(home.path()));
    }
    assert!(!config.to_string().contains("synthetic-token"));
}

#[test]
fn expired_and_excessive_grant_lifetimes_are_rejected() {
    ora_logging::initialize_test_clock();
    assert!(
        renewal_delay(chrono::Local::now().fixed_offset() - chrono::Duration::seconds(1)).is_err()
    );
    assert!(
        renewal_delay(chrono::Local::now().fixed_offset() + chrono::Duration::hours(1)).is_err()
    );
}

/// A CA file published to a workload must not contain a combined private-key PEM block.
#[test]
fn public_ca_material_rejects_private_key_blocks_before_grant_creation() {
    let root = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(root.path());
    let mut combined = std::fs::read(&gateway.config.ca_cert).unwrap();
    combined.extend(std::fs::read(&gateway.config.client_key).unwrap());
    std::fs::write(&gateway.config.ca_cert, combined).unwrap();
    let error = gateway.config.validate().unwrap_err();
    assert_eq!(error.to_string(), "model_proxy_tls_invalid");
    assert!(gateway.requests.lock().unwrap().is_empty());
}

#[test]
fn runtime_state_refuses_existing_files_and_symlinks_without_changing_them() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let grant = Grant {
        grant_id: "grant-1".into(),
        token: "synthetic-token".into(),
        expires_at: chrono::Local::now().fixed_offset(),
        protocol: ora_node_protocol::ModelProtocol::OpenaiCompletions,
        proxy_base_url: "https://gateway/runtime/openai/v1".into(),
        model: Model {
            id: "model".into(),
            name: "Model".into(),
            context_window: 32768,
            max_tokens: 1024,
        },
    };
    let runtime = root.path().join("model-runtime");
    std::fs::write(&runtime, b"existing user data").unwrap();
    assert!(opencode::environment(&grant, &root.path().join("ca.pem"), root.path()).is_err());
    assert_eq!(std::fs::read(&runtime).unwrap(), b"existing user data");
    std::fs::remove_file(&runtime).unwrap();
    symlink(checkout.path(), &runtime).unwrap();
    assert!(opencode::environment(&grant, &root.path().join("ca.pem"), root.path()).is_err());
    assert_eq!(std::fs::read_dir(checkout.path()).unwrap().count(), 0);
}

/// The frozen configuration the fixture gateway's grant serves.
fn adapter_configuration() -> FrozenAgentConfiguration {
    FrozenAgentConfiguration {
        plugin_id: ora_node_protocol::PluginId::new("official/ora-space.claude"),
        plugin_version: ora_node_protocol::PluginVersion::new("0.4.0"),
        target: "x86_64-unknown-linux-gnu".into(),
        package_sha256: ora_node_protocol::Sha256Digest::new("a".repeat(64)),
        schema_version: 2,
        declaration_fingerprint: ora_node_protocol::Sha256Digest::new("b".repeat(64)),
        adapter_version: 1,
        configuration_revision: 4,
        settings_json: r#"{"effort":"high"}"#.into(),
        protocol: ModelProtocol::OpenaiCompletions,
        model: ora_node_protocol::BoundModel {
            id: "team/model".into(),
            name: "Fixture".into(),
            context_window: 32768,
            max_tokens: 1024,
        },
    }
}

/// On the adapter route the Node writes no CLI configuration and keeps the token out of every
/// environment variable; only the startup context, which the runtime never persists, carries it.
#[tokio::test]
async fn adapter_route_hands_the_token_only_to_the_startup_context() {
    let root = tempfile::tempdir().unwrap();
    let gateway = Gateway::new(root.path());
    let configuration = adapter_configuration();
    let mut access = ModelAccess::open(
        gateway.config.clone(),
        &ModelBindingId::new("binding-1"),
        &ExecutionId::new("execution-1"),
        root.path(),
        ModelRoute::Adapter(&configuration),
    )
    .await
    .unwrap();
    let home = PathBuf::from(&access.environment["HOME"]);
    assert!(home.starts_with(root.path().join("model-runtime")));
    assert_eq!(
        access.environment.keys().cloned().collect::<Vec<_>>(),
        vec![
            "HOME",
            "NODE_EXTRA_CA_CERTS",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ]
    );
    assert!(
        access
            .environment
            .values()
            .all(|value| !value.contains("synthetic-temporary-token"))
    );
    let port = Url::parse(&gateway.config.gateway_url)
        .unwrap()
        .port()
        .unwrap();
    assert_eq!(
        access.context,
        Some(AgentModelContext {
            configuration: ora_plugin_protocol::AgentModelConfiguration {
                plugin_id: "official/ora-space.claude".into(),
                plugin_version: "0.4.0".into(),
                target: "x86_64-unknown-linux-gnu".into(),
                package_sha256: "a".repeat(64),
                schema_version: 2,
                declaration_fingerprint: "b".repeat(64),
                adapter_version: 1,
                configuration_revision: 4,
                settings_json: r#"{"effort":"high"}"#.into(),
                protocol: ora_plugin_protocol::AgentModelProtocol::OpenaiCompletions,
                model: ora_plugin_protocol::AgentBoundModel {
                    id: "team/model".into(),
                    name: "Fixture".into(),
                    context_window: 32768,
                    max_tokens: 1024,
                },
            },
            proxy_base_url: format!("https://localhost:{port}/runtime/openai/v1"),
            token: "synthetic-temporary-token".into(),
        })
    );
    access.close().await;
    drop(access);
    assert!(!home.exists());
    assert_eq!(
        gateway
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(route, _)| route.clone())
            .filter(|route| !route.contains("/renew "))
            .collect::<Vec<_>>(),
        vec![
            "POST /internal/v1/model-grants HTTP/1.1",
            "DELETE /internal/v1/model-grants/grant-1 HTTP/1.1"
        ]
    );
}

/// A grant that serves another model or protocol than the frozen configuration is revoked before
/// any CLI state exists: the binding and the admitted configuration disagree.
#[tokio::test]
async fn adapter_route_revokes_a_grant_that_differs_from_the_frozen_configuration() {
    let mismatches = [
        FrozenAgentConfiguration {
            protocol: ModelProtocol::AnthropicMessages,
            ..adapter_configuration()
        },
        FrozenAgentConfiguration {
            model: ora_node_protocol::BoundModel {
                id: "team/other".into(),
                ..adapter_configuration().model
            },
            ..adapter_configuration()
        },
        FrozenAgentConfiguration {
            model: ora_node_protocol::BoundModel {
                max_tokens: 2048,
                ..adapter_configuration().model
            },
            ..adapter_configuration()
        },
    ];
    for configuration in mismatches {
        let root = tempfile::tempdir().unwrap();
        let gateway = Gateway::new(root.path());
        let result = ModelAccess::open(
            gateway.config.clone(),
            &ModelBindingId::new("binding-1"),
            &ExecutionId::new("execution-1"),
            root.path(),
            ModelRoute::Adapter(&configuration),
        )
        .await;
        let Err(error) = result else {
            panic!("a mismatched grant must not start an adapter")
        };
        assert_eq!(error.to_string(), "model_grant_mismatch");
        assert!(!root.path().join("model-runtime").exists());
        assert_eq!(
            gateway
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|(route, _)| route.clone())
                .collect::<Vec<_>>(),
            vec![
                "POST /internal/v1/model-grants HTTP/1.1",
                "DELETE /internal/v1/model-grants/grant-1 HTTP/1.1"
            ]
        );
    }
}
