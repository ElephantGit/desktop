//! Records the marketplace sources one registry-index rebuild could not refresh.
//!
//! A rebuild must not drop a failing source's listings: a repository that is temporarily
//! unreachable would look like one that withdrew its plugins. The rebuild therefore keeps the
//! listings the previous index already carried and records the failure beside them, and that
//! record is persisted with the cache because the marketplace page reads the cache rather than
//! the sync response that produced it.

use ora_utils::url::canonical_repository_url;
use serde::{Deserialize, Serialize};

/// Describes one marketplace source whose refresh failed while the index was rebuilt.
///
/// The URL is canonicalized on construction because it is matched against the canonical URL every
/// index entry is attributed to: a source that failed must be recognized no matter which spelling
/// of its URL the caller used.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RegistrySourceFailure {
    url: String,
    message: String,
}

impl RegistrySourceFailure {
    /// Records one failed source and the reason its refresh failed.
    pub fn new(url: impl AsRef<str>, message: impl Into<String>) -> Self {
        Self {
            url: canonical_repository_url(url.as_ref()),
            message: message.into(),
        }
    }

    /// Returns the canonical URL of the source that could not be refreshed.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the failure description shown to the user and written to the log.
    pub fn message(&self) -> &str {
        &self.message
    }
}
