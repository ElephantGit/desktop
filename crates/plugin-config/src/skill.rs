//! Resolves one complete Skill configuration into a readable process environment.

use crate::{ConfigurationCompleteness, ConfigurationDetails, ConfigurationSummary};
use std::collections::BTreeMap;

/// Separates unusable configuration from valid values whose names collide, without carrying values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillEnvironmentError {
    /// Missing, incomplete or OS-incompatible values cannot be injected; their declarations still own names.
    InvalidConfiguration,
    /// Valid values still participate in global collision detection, but cannot be injected.
    NameCollision,
}

/// Resolves all values atomically; incomplete or OS-incompatible configurations yield no variables.
pub fn resolve_skill_environment(
    plugin_id: &str,
    details: &ConfigurationDetails,
) -> Result<BTreeMap<String, String>, SkillEnvironmentError> {
    if details.summary
        != (ConfigurationSummary::Available {
            completeness: ConfigurationCompleteness::Complete,
        })
    {
        return Err(SkillEnvironmentError::InvalidConfiguration);
    }
    let mut environment = BTreeMap::new();
    let mut collision = false;
    for setting in &details.settings {
        if setting.value_error_code.is_some() {
            return Err(SkillEnvironmentError::InvalidConfiguration);
        }
        if let Some(value) = &setting.effective_value {
            let value = value.as_runtime_text();
            if value.contains('\0') {
                return Err(SkillEnvironmentError::InvalidConfiguration);
            }
            if environment
                .insert(
                    skill_environment_key(plugin_id, &setting.declaration.id),
                    value,
                )
                .is_some()
            {
                collision = true;
            }
        }
    }
    if collision {
        Err(SkillEnvironmentError::NameCollision)
    } else {
        Ok(environment)
    }
}

/// Uses ASCII uppercase alphanumerics and replaces each other Unicode scalar with one underscore.
pub fn skill_environment_key(plugin_id: &str, setting_id: &str) -> String {
    let mut key = String::from("ORA_SKILL_");
    for (index, part) in [plugin_id, setting_id].into_iter().enumerate() {
        if index > 0 {
            key.push('_');
        }
        key.extend(part.chars().map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        }));
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConfigurationService;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Unknown aliases cannot override ordinary inherited environment variables.
    #[test]
    fn rejects_environment_aliases() {
        for member in ["env", "environment", "environmentVariable", "name"] {
            let mut declaration = json!({"schemaVersion":1,"settings":{"token":{
                "type":"string","title":"Token","description":"Token"
            }}});
            declaration["settings"]["token"][member] = json!("GITHUB_TOKEN");
            assert!(crate::compile_declaration(declaration.to_string().as_bytes()).is_err());
        }
    }

    /// A collision rejects even the other noncolliding fields in this package.
    #[test]
    fn rejects_same_package_collisions() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("package");
        std::fs::create_dir_all(package.join("assets")).unwrap();
        for pair in [["tokenA", "tokena"], ["aValue", "avalue"]] {
            let setting =
                json!({"type":"string","title":"Value","description":"Value","default":"private"});
            let mut settings = serde_json::Map::new();
            for id in [pair[0], pair[1], "unrelated"] {
                settings.insert(id.into(), setting.clone());
            }
            std::fs::write(
                package.join("assets/config.json"),
                json!({"schemaVersion":1,"settings":settings}).to_string(),
            )
            .unwrap();
            let details = ConfigurationService::new(root.path())
                .get("official/demo", &package)
                .unwrap()
                .unwrap();
            assert_eq!(
                resolve_skill_environment("official/demo", &details),
                Err(SkillEnvironmentError::NameCollision)
            );
        }
    }

    /// Drives declaration parsing, defaults, completeness and serialization through the real store.
    #[test]
    fn resolves_complete_scalars_and_rejects_whole_incomplete_or_nul_packages() {
        let root = tempfile::tempdir().unwrap();
        let assets = root.path().join("package").join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        let mut declaration = json!({"schemaVersion":1,"settings":{
            "token":{"type":"string","title":"Token","description":"Credential","secret":true,"default":"private"},
            "count":{"type":"number","title":"Count","description":"Count","default":3},
            "enabled":{"type":"boolean","title":"Enabled","description":"Enabled","default":false},
            "optional":{"type":"string","title":"Optional","description":"Optional"}
        }});
        let service = ConfigurationService::new(root.path());
        for invalid in [false, true] {
            declaration["settings"]["optional"]["required"] = json!(invalid);
            std::fs::write(assets.join("config.json"), declaration.to_string()).unwrap();
            let details = service
                .get("official/demo", assets.parent().unwrap())
                .unwrap()
                .unwrap();
            assert!(
                details
                    .settings
                    .iter()
                    .find(|s| s.declaration.id == "token")
                    .unwrap()
                    .declaration
                    .secret
            );
            assert_eq!(
                resolve_skill_environment("official/demo", &details),
                if invalid {
                    Err(SkillEnvironmentError::InvalidConfiguration)
                } else {
                    Ok(BTreeMap::from([
                        (
                            skill_environment_key("official/demo", "token"),
                            "private".into(),
                        ),
                        (skill_environment_key("official/demo", "count"), "3".into()),
                        (
                            skill_environment_key("official/demo", "enabled"),
                            "false".into(),
                        ),
                    ]))
                }
            );
        }
        declaration["settings"]["optional"]["required"] = json!(false);
        declaration["settings"]["token"]["default"] = json!("private\0value");
        std::fs::write(assets.join("config.json"), declaration.to_string()).unwrap();
        let details = service
            .get("official/demo", assets.parent().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            resolve_skill_environment("official/demo", &details),
            Err(SkillEnvironmentError::InvalidConfiguration)
        );
    }

    /// Readable names follow the portable ASCII rule; secret still participates in fingerprints.
    #[test]
    fn readable_names_and_secret_changes_affect_fingerprints() {
        assert_eq!(
            skill_environment_key("official/demo", "token"),
            "ORA_SKILL_OFFICIAL_DEMO_TOKEN"
        );
        let keys = [("a/b", "token"), ("a-b", "token"), ("a/b", "Token")]
            .map(|(plugin, setting)| skill_environment_key(plugin, setting));
        assert_eq!(
            keys.iter().collect::<std::collections::BTreeSet<_>>().len(),
            1
        );
        assert_eq!(
            skill_environment_key("a/b-.é中", "x__9"),
            "ORA_SKILL_A_B_____X__9"
        );
        let plain = br#"{"schemaVersion":1,"settings":{"token":{"type":"string","title":"Token","description":"Token"}}}"#;
        let mut secret: serde_json::Value = serde_json::from_slice(plain).unwrap();
        secret["settings"]["token"]["secret"] = json!(true);
        let old = crate::compile_declaration(plain).unwrap();
        let new = crate::compile_declaration(secret.to_string().as_bytes()).unwrap();
        assert!(!old.settings[0].secret);
        assert!(new.settings[0].secret);
        assert_ne!(old.fingerprint, new.fingerprint);
    }
}
