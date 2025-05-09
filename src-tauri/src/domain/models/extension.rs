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

/// Extension manifest struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    /// Display name of the extension
    pub display_name: String,
    /// Version of the extension
    pub version: String,
    /// Author of the extension
    pub author: String,
    /// Description of the extension
    #[serde(default)]
    pub description: String,
    /// Main JavaScript file
    #[serde(default)]
    pub js: Option<String>,
    /// CSS file
    #[serde(default)]
    pub css: Option<String>,
    /// Required modules
    #[serde(default)]
    pub requires: Vec<String>,
    /// Optional modules
    #[serde(default)]
    pub optional: Vec<String>,
    /// Loading order
    #[serde(default = "default_loading_order")]
    pub loading_order: i32,
    /// Whether to auto-update the extension
    #[serde(default)]
    pub auto_update: bool,
    /// Generate interceptor function name
    #[serde(default)]
    pub generate_interceptor: Option<String>,
    /// Localization data
    #[serde(default)]
    pub i18n: std::collections::HashMap<String, String>,
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
    /// Manifest of the extension
    pub manifest: Option<ExtensionManifest>,
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
