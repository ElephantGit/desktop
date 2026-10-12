//! Deployment-provided git-access client identity for the platform Git Server.
//!
//! The Sandbox Server issues this certificate for the runtime's tenant, Workspace and generation,
//! separately from the Node WSS server identity and the model-access identity. The certificate
//! carries no repository permission: the Git Server authorizes every request from current Cloud
//! state and the execution the request declares. Only the Node process presents it, per command,
//! so the private key never enters a checkout, Git configuration, log or journal.

#[cfg(test)]
mod tests;

use reqwest::{Certificate, Identity};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Absolute paths of the git-access material written by the entrypoint into management tmpfs.
/// The Git Server address is not deployment configuration: each execution input supplies it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitAccessConfig {
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

/// Errors expose bounded codes only; certificate contents and paths are never error messages.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct GitAccessError(&'static str);

impl GitAccessConfig {
    /// Rejects unusable material at startup so a misconfigured Node fails closed before it
    /// accepts work, instead of discovering it during a delivery push.
    pub(crate) fn validate(&self) -> Result<(), GitAccessError> {
        if [&self.ca_cert, &self.client_cert, &self.client_key]
            .into_iter()
            .any(|path| !path.is_absolute())
        {
            return Err(GitAccessError("git_access_configuration_invalid"));
        }
        let read = |path: &PathBuf| {
            std::fs::read(path).map_err(|_| GitAccessError("git_access_tls_invalid"))
        };
        let ca = read(&self.ca_cert)?;
        // A trust anchor must never double as a key file; a combined CA/key file fails closed.
        if ca
            .windows(b"PRIVATE KEY".len())
            .any(|part| part == b"PRIVATE KEY")
        {
            return Err(GitAccessError("git_access_tls_invalid"));
        }
        Certificate::from_pem(&ca).map_err(|_| GitAccessError("git_access_tls_invalid"))?;
        let mut identity = read(&self.client_cert)?;
        identity.push(b'\n');
        identity.extend(read(&self.client_key)?);
        Identity::from_pem(&identity).map_err(|_| GitAccessError("git_access_tls_invalid"))?;
        Ok(())
    }
}
