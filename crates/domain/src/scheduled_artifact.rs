use serde::{Deserialize, Serialize};

/// The fixed worktree directory an agent declares its Scheduled Run artifacts in.
///
/// The directory is a documented, optional convention the user writes into the task prompt;
/// the runtime only collects from it and never injects it into the prompt.
pub const SCHEDULED_ARTIFACT_DIRECTORY: &str = ".ora-scheduled-artifacts";

/// Upper bound on one collected artifact, in bytes (10 MiB).
pub const MAX_SCHEDULED_ARTIFACT_BYTES: u64 = 10 * 1024 * 1024;

/// Upper bound on the number of artifacts collected from one run.
pub const MAX_SCHEDULED_ARTIFACTS_PER_RUN: usize = 64;

/// Upper bound on the total bytes of all artifacts collected from one run (64 MiB).
pub const MAX_SCHEDULED_ARTIFACT_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// Upper bound on how many finished runs keep their artifacts per task.
pub const MAX_SCHEDULED_ARTIFACT_RUNS_PER_TASK: usize = 10;

/// The closed set of artifact types an agent may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledArtifactKind {
    Markdown,
    Json,
    Text,
    Log,
    Xml,
    Html,
}

impl ScheduledArtifactKind {
    /// Returns the stable contract and column representation of this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Json => "json",
            Self::Text => "text",
            Self::Log => "log",
            Self::Xml => "xml",
            Self::Html => "html",
        }
    }

    /// Derives the artifact kind from a file name's extension, or `None` when the extension
    /// is not in the whitelist. Matching is case-insensitive.
    pub fn from_file_name(name: &str) -> Option<Self> {
        let extension = name.rsplit('.').next()?;
        match extension.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            "txt" => Some(Self::Text),
            "log" => Some(Self::Log),
            "xml" => Some(Self::Xml),
            "html" => Some(Self::Html),
            _ => None,
        }
    }

    /// Parses the case-insensitive contract token back into a kind.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "markdown" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            "text" => Some(Self::Text),
            "log" => Some(Self::Log),
            "xml" => Some(Self::Xml),
            "html" => Some(Self::Html),
            _ => None,
        }
    }
}

/// Explains why one candidate artifact was not collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledArtifactRejectionReason {
    /// The file name's extension is outside the closed whitelist.
    UnsupportedType,
    /// The relative path is unsafe (traversal, absolute, drive, control characters, etc.).
    PathUnsafe,
    /// The candidate is a symlink rather than a regular file.
    Symlink,
    /// The candidate is a hard link with more than one name, so its inode may be reachable
    /// outside the artifact directory.
    HardLink,
    /// The file exceeds the single-artifact size bound.
    TooLarge,
    /// The run already collected the per-run artifact count bound.
    TooMany,
    /// The run already collected the per-run total byte bound.
    TotalTooLarge,
    /// The file could not be read.
    ReadFailed,
    /// The file could not be copied into artifact storage.
    CopyFailed,
}

impl ScheduledArtifactRejectionReason {
    /// Returns the stable column and contract representation of this reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedType => "unsupported_type",
            Self::PathUnsafe => "path_unsafe",
            Self::Symlink => "symlink",
            Self::HardLink => "hard_link",
            Self::TooLarge => "too_large",
            Self::TooMany => "too_many",
            Self::TotalTooLarge => "total_too_large",
            Self::ReadFailed => "read_failed",
            Self::CopyFailed => "copy_failed",
        }
    }

    /// Parses the stable column token back into a rejection reason.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unsupported_type" => Some(Self::UnsupportedType),
            "path_unsafe" => Some(Self::PathUnsafe),
            "symlink" => Some(Self::Symlink),
            "hard_link" => Some(Self::HardLink),
            "too_large" => Some(Self::TooLarge),
            "too_many" => Some(Self::TooMany),
            "total_too_large" => Some(Self::TotalTooLarge),
            "read_failed" => Some(Self::ReadFailed),
            "copy_failed" => Some(Self::CopyFailed),
            _ => None,
        }
    }
}

/// One collected artifact and the metadata registered in the per-run manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledRunArtifact {
    /// Forward-slash path relative to [`SCHEDULED_ARTIFACT_DIRECTORY`], safe to join under storage.
    pub relative_path: String,
    pub kind: ScheduledArtifactKind,
    pub size_bytes: u64,
    pub registered_at: i64,
}

/// One candidate artifact the run refused to collect, kept for history visibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledArtifactRejection {
    pub name: String,
    pub reason: ScheduledArtifactRejectionReason,
}

#[cfg(test)]
mod tests {
    use super::ScheduledArtifactKind;

    #[test]
    fn derives_the_whitelisted_kind_from_a_file_extension() {
        for (name, kind) in [
            ("report.md", ScheduledArtifactKind::Markdown),
            ("REPORT.MARKDOWN", ScheduledArtifactKind::Markdown),
            ("result.json", ScheduledArtifactKind::Json),
            ("notes.txt", ScheduledArtifactKind::Text),
            ("run.log", ScheduledArtifactKind::Log),
            ("coverage.xml", ScheduledArtifactKind::Xml),
            ("index.html", ScheduledArtifactKind::Html),
        ] {
            assert_eq!(ScheduledArtifactKind::from_file_name(name), Some(kind));
        }
    }

    #[test]
    fn rejects_extensions_outside_the_whitelist() {
        for name in ["report.pdf", "script.sh", "noextension", "malware.exe"] {
            assert_eq!(ScheduledArtifactKind::from_file_name(name), None);
        }
    }

    #[test]
    fn parses_the_stable_contract_token() {
        assert_eq!(
            ScheduledArtifactKind::parse("markdown"),
            Some(ScheduledArtifactKind::Markdown)
        );
        assert_eq!(
            ScheduledArtifactKind::parse("JSON"),
            Some(ScheduledArtifactKind::Json)
        );
        assert_eq!(ScheduledArtifactKind::parse("bogus"), None);
    }
}
