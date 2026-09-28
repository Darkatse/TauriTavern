use serde::{Deserialize, Serialize};

/// Extension type enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionType {
    /// Built-in extension
    System,
    /// User-installed extension (local)
    Local,
    /// Global extension (available to all users)
    Global,
}

/// An extension discovered in the application or user directories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extension {
    /// Name of the extension (folder name)
    pub name: String,
    /// Type of the extension
    pub extension_type: ExtensionType,
}

/// Extension version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionVersion {
    /// Current branch name
    pub current_branch_name: String,
    /// Current commit hash
    pub current_commit_hash: String,
    /// Whether the extension is up to date
    pub is_up_to_date: bool,
    /// Remote URL of the extension repository
    pub remote_url: String,
}

/// A remote branch available to an installed extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionBranch {
    /// Remote branch name without the `refs/heads/` prefix.
    pub name: String,
    /// Short commit hash for display.
    pub commit: String,
    /// Whether the extension currently selects this branch.
    pub current: bool,
    /// Optional display label. Remote advertisement does not provide one.
    pub label: String,
}

/// Extension installation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionInstallResult {
    /// Version of the extension
    pub version: String,
    /// Author of the extension
    pub author: String,
    /// Display name of the extension
    pub display_name: String,
    /// Path to the extension
    pub extension_path: String,
    /// Installed extension folder name
    pub folder_name: String,
}

/// Extension update result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionUpdateResult {
    /// Short commit hash
    pub short_commit_hash: String,
    /// Path to the extension
    pub extension_path: String,
    /// Whether the extension is up to date
    pub is_up_to_date: bool,
    /// Remote URL of the extension repository
    pub remote_url: String,
}
