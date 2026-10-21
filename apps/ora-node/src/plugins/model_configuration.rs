//! Management-owned package receipts bind a configured session to verified archive bytes.
//!
//! Cloud freezes a package digest, a declaration fingerprint and Settings at admission. Between
//! admission and start the same version directory can be republished, and a frozen configuration
//! can belong to another declaration; neither is visible to a capability check. Before any model
//! authority is handed out, the session re-proves all of it against what is installed.

use super::DirectoryPluginCatalog;
use crate::session::InstalledConfigurationError;
use ora_node_protocol::{FrozenAgentConfiguration, ModelProtocol};
use std::io::Write;

/// The downloaded artifact's verified identity, selected by the installer before any reuse.
pub(super) struct PackageArtifact<'a> {
    pub(super) sha256: &'a str,
    pub(super) target: &'a str,
}

impl DirectoryPluginCatalog {
    /// Receipts live outside package content, so an archive cannot supply its own proof.
    fn receipt_path(
        &self,
        id: &ora_domain::PluginId,
        version: &semver::Version,
    ) -> std::path::PathBuf {
        self.home
            .join("plugins")
            .join(".node-receipts")
            .join(id.namespace())
            .join(id.name())
            .join(format!("{version}.json"))
    }

    /// Retires proof before package mutation, so a failed new receipt cannot certify new bytes
    /// as old.
    pub(super) fn clear_package_record(
        &self,
        id: &ora_domain::PluginId,
        version: &semver::Version,
    ) -> std::io::Result<()> {
        let path = self.receipt_path(id, version);
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("invalid receipt path"))?;
        match ora_utils::path::check_directory_without_symlinks(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
        match std::fs::symlink_metadata(&path) {
            Ok(_) => std::fs::remove_file(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Called only after digest-verified archive publication, under the catalog's use lock.
    pub(super) fn record_package(
        &self,
        id: &ora_domain::PluginId,
        version: &semver::Version,
        artifact: &PackageArtifact<'_>,
    ) -> std::io::Result<()> {
        let path = self.receipt_path(id, version);
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("invalid receipt path"))?;
        ora_utils::path::create_directories_without_symlinks(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&serde_json::to_vec(&receipt(artifact))?)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    /// Absence is unproven legacy state, never permission to adopt the current claimed digest.
    pub(super) fn package_recorded(
        &self,
        id: &ora_domain::PluginId,
        version: &semver::Version,
        artifact: &PackageArtifact<'_>,
    ) -> bool {
        let path = self.receipt_path(id, version);
        if path
            .parent()
            .is_none_or(|parent| ora_utils::path::check_directory_without_symlinks(parent).is_err())
        {
            return false;
        }
        if !std::fs::symlink_metadata(&path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() < 4096)
        {
            return false;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .is_ok_and(|value| value == receipt(artifact))
    }

    /// Re-proves the frozen package, declaration and Settings while the session holds its lease,
    /// and that the declaration offers the frozen protocol.
    pub(super) fn check_installed_configuration(
        &self,
        configuration: &FrozenAgentConfiguration,
    ) -> Result<(), InstalledConfigurationError> {
        use InstalledConfigurationError::{Invalid, ProtocolMismatch};
        let (id, version) =
            super::catalog::identity(&configuration.plugin_id, &configuration.plugin_version)
                .map_err(|_| Invalid)?;
        let target = ora_plugin_registry::current_host_target().ok_or(Invalid)?;
        let artifact = PackageArtifact {
            sha256: configuration.package_sha256.as_str(),
            target: target.as_str(),
        };
        if target.as_str() != configuration.target
            || !self.package_recorded(&id, &version, &artifact)
        {
            return Err(Invalid);
        }
        let declaration = ora_plugin_manager::read_agent_declaration(&self.path(&id, &version))
            .map_err(|_| Invalid)?;
        let settings = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
            &configuration.settings_json,
        )
        .map_err(|_| Invalid)?;
        if declaration.fingerprint != configuration.declaration_fingerprint.as_str()
            || declaration.schema_version != configuration.schema_version
            || declaration.model_access.contract_version != configuration.adapter_version
            || !ora_plugin_config::validate_agent_settings(&declaration, &settings)
        {
            return Err(Invalid);
        }
        let declared = declaration
            .model_access
            .protocols
            .iter()
            .any(|protocol| match protocol {
                ora_plugin_config::AgentModelProtocol::OpenaiCompletions => {
                    configuration.protocol == ModelProtocol::OpenaiCompletions
                }
                ora_plugin_config::AgentModelProtocol::AnthropicMessages => {
                    configuration.protocol == ModelProtocol::AnthropicMessages
                }
            });
        if !declared {
            return Err(ProtocolMismatch);
        }
        Ok(())
    }
}

/// The exact receipt content; any other shape proves nothing.
fn receipt(artifact: &PackageArtifact<'_>) -> serde_json::Value {
    serde_json::json!({ "sha256": artifact.sha256, "target": artifact.target })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ora_node_protocol::{BoundModel, PluginId, PluginVersion, Sha256Digest};
    use pretty_assertions::assert_eq;

    const DECLARATION: &[u8] = br#"{"schemaVersion":2,"agent":{"modelAccess":{"contractVersion":1,"protocols":["anthropic-messages"],"modelCardinality":"single"}},"settings":{"region":{"type":"string","title":"Region","description":"Region","required":true}}}"#;

    /// Archive proof, exact declaration, Settings and protocol are all required, even for a
    /// third-party plugin id; replacing the package after install withdraws the proof.
    #[test]
    fn installed_configuration_requires_receipt_declaration_and_settings() {
        let root = tempfile::tempdir().expect("Node home");
        let catalog = DirectoryPluginCatalog::new(root.path().to_path_buf());
        let id = ora_domain::PluginId::parse("vendor/minimal").expect("plugin id");
        let version = semver::Version::new(1, 0, 0);
        let Some(target) = ora_plugin_registry::current_host_target() else {
            return;
        };
        let assets = catalog.path(&id, &version).join("assets");
        std::fs::create_dir_all(&assets).expect("package");
        std::fs::write(assets.join("config.json"), DECLARATION).expect("declaration");
        let fingerprint = ora_utils::hash::sha256_hex(DECLARATION);
        let mut configuration = FrozenAgentConfiguration {
            plugin_id: PluginId::new(id.canonical()),
            plugin_version: PluginVersion::new(version.to_string()),
            target: target.as_str().into(),
            package_sha256: Sha256Digest::new("a".repeat(64)),
            schema_version: 2,
            declaration_fingerprint: Sha256Digest::new(fingerprint.clone()),
            adapter_version: 1,
            configuration_revision: 1,
            settings_json: r#"{"region":"eu"}"#.into(),
            protocol: ModelProtocol::AnthropicMessages,
            model: BoundModel {
                id: "vendor/model".into(),
                name: "Model".into(),
                context_window: 128_000,
                max_tokens: 8192,
            },
        };
        let invalid = Err(InstalledConfigurationError::Invalid);
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            invalid
        );

        let digest = "a".repeat(64);
        let artifact = PackageArtifact {
            sha256: &digest,
            target: target.as_str(),
        };
        catalog
            .record_package(&id, &version, &artifact)
            .expect("verified receipt");
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            Ok(())
        );

        configuration.protocol = ModelProtocol::OpenaiCompletions;
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            Err(InstalledConfigurationError::ProtocolMismatch)
        );
        configuration.protocol = ModelProtocol::AnthropicMessages;

        configuration.settings_json = r#"{"region":"eu","unexpected":true}"#.into();
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            invalid
        );
        configuration.settings_json = "{}".into();
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            invalid
        );
        configuration.settings_json = r#"{"region":"eu"}"#.into();

        configuration.declaration_fingerprint = Sha256Digest::new("c".repeat(64));
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            invalid
        );
        configuration.declaration_fingerprint = Sha256Digest::new(fingerprint);

        configuration.package_sha256 = Sha256Digest::new("b".repeat(64));
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            invalid
        );
        configuration.package_sha256 = Sha256Digest::new(digest.clone());

        // Republishing clears the old proof before the package bytes change.
        catalog
            .clear_package_record(&id, &version)
            .expect("retired receipt");
        assert_eq!(
            catalog.check_installed_configuration(&configuration),
            invalid
        );
    }
}
