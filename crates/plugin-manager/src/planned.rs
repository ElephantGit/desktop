//! Installs a self-contained plan whose identity and digest come from an external authority.
//! Package interpretation stays here; Node supplies the plan and owns use leases and recovery.
use crate::{InstallError, Installer, ResolvedReleaseSource, installed_root};
use ora_domain::{PluginId, PluginNamespace};
use ora_plugin_manifest::PluginKind;
use ora_utils::{
    archive::{ArchiveFormat, extract_archive},
    http::{Checksum, DownloadOptions, DownloadRequest, HttpDownload},
    path::create_directories_without_symlinks,
};
use semver::Version;
use std::path::{Path, PathBuf};

/// Fully verified package, invisible to discovery until its owner commits it under a use lock.
pub struct PreparedPlugin {
    staging: tempfile::TempDir,
    id: PluginId,
    version: Version,
}

/// Validates exactly one package, including its identity; no highest-version selection is made.
pub fn inspect_planned_package(
    path: &Path,
    id: &PluginId,
    version: &Version,
) -> Result<(), InstallError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| io_error(path, source))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(InstallError::invalid_field(
            "package",
            "package must be a directory, not a link",
        ));
    }
    let manifest = crate::install::package::read_installed_manifest(path)?;
    if manifest.name().as_str() != id.name() || manifest.version() != version {
        return Err(InstallError::invalid_field(
            "identity",
            "package differs from the planned identity",
        ));
    }
    let namespace = PluginNamespace::parse(id.namespace())
        .map_err(|e| InstallError::invalid_field("namespace", e.to_string()))?;
    crate::validation::validate(path, &manifest, &namespace, /*logo*/ None)
        .map_err(InstallError::invalid_package)?;
    Ok(())
}

impl<D: HttpDownload> Installer<D> {
    /// Downloads with the caller's bounded policy, verifies the digest, and stages a checked
    /// package. No marketplace metadata is needed and nothing reaches the installed root yet.
    pub async fn prepare_release(
        &self,
        id: &PluginId,
        version: &Version,
        source: ResolvedReleaseSource,
        staging_root: &Path,
        options: DownloadOptions,
    ) -> Result<PreparedPlugin, InstallError> {
        create_directories_without_symlinks(staging_root).map_err(|e| io_error(staging_root, e))?;
        let staging = tempfile::Builder::new()
            .prefix("install-")
            .tempdir_in(staging_root)
            .map_err(|e| io_error(staging_root, e))?;
        let archive = staging.path().join("release.orax");
        self.downloader
            .download(DownloadRequest {
                source: source.download().clone(),
                destination: archive.clone(),
                checksum: Some(Checksum::sha256(source.sha256().to_vec())),
                options,
                progress: None,
                cancel: None,
            })
            .await?;
        // Recheck at this trust boundary even for injected downloaders. No archive is extracted
        // until the authority's digest is proven against the actual file.
        let actual = ora_utils::hash::sha256_file(&archive).map_err(|e| io_error(&archive, e))?;
        let expected: String = source.sha256().iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected {
            return Err(InstallError::ChecksumMismatch { expected, actual });
        }
        let package = staging.path().join("package");
        std::fs::create_dir(&package).map_err(|e| io_error(&package, e))?;
        extract_archive(
            ArchiveFormat::Zip,
            &archive,
            &package,
            &crate::limits::package_extract_limits(),
        )
        .map_err(|source| InstallError::Extract {
            path: package.clone(),
            source,
        })?;
        inspect_planned_package(&package, id, version)?;
        let manifest = crate::install::package::read_installed_manifest(&package)?;
        if let Some(selected) = source.target() {
            let artifact = manifest
                .artifact()
                .ok_or(InstallError::MissingArtifactTarget)?;
            if artifact.target() != selected {
                return Err(InstallError::TargetMismatch {
                    release: selected.to_string(),
                    artifact: artifact.target().to_string(),
                });
            }
        } else if manifest.artifact().is_some() || matches!(manifest.kind(), PluginKind::Hook) {
            // A native package cannot masquerade as a target-independent plan.
            return Err(InstallError::MissingArtifactTarget);
        }
        Ok(PreparedPlugin {
            staging,
            id: id.clone(),
            version: version.clone(),
        })
    }
}

impl PreparedPlugin {
    /// Publishes only verified bytes. The caller excludes session leases while replacing or
    /// removing packages. A corrupt same-version tree is retained in staging until commit succeeds.
    pub fn commit(self, data_dir: &Path) -> Result<PathBuf, InstallError> {
        let parent = installed_root(data_dir)
            .join(self.id.namespace())
            .join(self.id.name());
        create_directories_without_symlinks(&parent).map_err(|e| io_error(&parent, e))?;
        let destination = parent.join(self.version.to_string());
        let old = self.staging.path().join("previous");
        let exists = match std::fs::symlink_metadata(&destination) {
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => true,
            Ok(_) => {
                return Err(InstallError::invalid_field(
                    "package",
                    "refusing to replace a link or non-directory",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(io_error(&destination, e)),
        };
        if exists {
            std::fs::rename(&destination, &old).map_err(|e| io_error(&destination, e))?;
        }
        if let Err(error) = std::fs::rename(self.staging.path().join("package"), &destination) {
            if exists {
                std::fs::rename(&old, &destination).map_err(|e| io_error(&destination, e))?;
            }
            return Err(io_error(&destination, error));
        }
        Ok(destination)
    }
}

/// Attaches the failing path without introducing Node error vocabulary into the installer.
fn io_error(path: &Path, source: std::io::Error) -> InstallError {
    InstallError::Io {
        path: path.to_path_buf(),
        source,
    }
}
