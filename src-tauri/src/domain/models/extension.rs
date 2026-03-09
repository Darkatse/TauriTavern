use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

/// Backend-facing extension manifest summary.
///
/// This intentionally keeps only the metadata that the Rust installation and discovery
/// pipeline actually needs. Frontend runtime fields such as `js`, `css`, `i18n`, and
/// other browser-loading semantics are loaded from the raw `manifest.json` by the web
/// runtime instead of being re-modeled here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionManifestMetadata {
    /// Display name of the extension
    pub display_name: String,
    /// Version of the extension
    pub version: String,
    /// Author of the extension
    pub author: String,
    /// Description of the extension
    #[serde(default)]
    pub description: String,
    /// Loading order
    #[serde(default = "default_loading_order")]
    pub loading_order: i32,
}

fn default_loading_order() -> i32 {
    100
}

/// Extension struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extension {
    /// Name of the extension (folder name)
    pub name: String,
    /// Type of the extension
    pub extension_type: ExtensionType,
    /// Whether the extension is managed by TauriTavern.
    ///
    /// Managed extensions have source metadata and can be updated.
    /// Unmanaged extensions are still discoverable/loadable but cannot be updated.
    pub managed: bool,
    /// Backend-facing manifest summary of the extension
    pub manifest: Option<ExtensionManifestMetadata>,
    /// Path to the extension
    pub path: PathBuf,
    /// Remote URL of the extension repository
    pub remote_url: Option<String>,
    /// Current commit hash
    pub commit_hash: Option<String>,
    /// Current branch name
    pub branch_name: Option<String>,
    /// Whether the extension is up to date
    pub is_up_to_date: Option<bool>,
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
