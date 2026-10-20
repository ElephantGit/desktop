//! OpenCode config uses environment references so serialization cannot persist the runtime token.

use super::{Grant, ModelAccessError};
use ora_node_protocol::ModelProtocol;
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

/// Builds one provider and isolated CLI state while leaving model identifiers unmodified.
pub(super) fn environment(
    grant: &Grant,
    ca: &Path,
    node_home: &Path,
) -> Result<(BTreeMap<String, String>, tempfile::TempDir), ModelAccessError> {
    let home = super::isolated_home(node_home, "opencode-")?;
    let model_id = &grant.model.id;
    let config = json!({
        "$schema": "https://opencode.ai/config.json",
        "model": format!("ora-model/{model_id}"),
        "small_model": format!("ora-model/{model_id}"),
        "enabled_providers": ["ora-model"],
        "provider": {"ora-model": {
            "npm": match grant.protocol {
                ModelProtocol::OpenaiCompletions => "@ai-sdk/openai-compatible",
                ModelProtocol::AnthropicMessages => "@ai-sdk/anthropic",
            },
            "name": "Ora model gateway",
            "options": {"baseURL": grant.proxy_base_url, "apiKey": "{env:ORA_MODEL_ACCESS_TOKEN}"},
            "models": {model_id: {
                "name": grant.model.name,
                "limit": {"context": grant.model.context_window, "output": grant.model.max_tokens},
            }},
        }},
    });
    let mut variables = super::home_variables(home.path(), ca);
    variables.extend([
        ("OPENCODE_CONFIG_CONTENT".to_string(), config.to_string()),
        ("ORA_MODEL_ACCESS_TOKEN".to_string(), grant.token.clone()),
        (
            "OPENCODE_DISABLE_AUTOUPDATE".to_string(),
            "true".to_string(),
        ),
        (
            "OPENCODE_DISABLE_MODELS_FETCH".to_string(),
            "true".to_string(),
        ),
        // Keep CLI dependency installation and private state outside the checkout/Revision.
        (
            "OPENCODE_DISABLE_PROJECT_CONFIG".to_string(),
            "true".to_string(),
        ),
    ]);
    // Disallow discovery of a user's global config and credentials on this management host.
    variables.insert(
        "OPENCODE_CONFIG_DIR".to_string(),
        home.path().join("config").to_string_lossy().into_owned(),
    );
    Ok((variables, home))
}
