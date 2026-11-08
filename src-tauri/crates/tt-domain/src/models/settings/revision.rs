use serde::{Deserialize, Serialize};

/// Identifies the stored user settings for save conflicts. The settings store derives it from what it
/// stores; everyone else only compares it and hands it back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserSettingsRevision {
    pub hash_algorithm: String,
    pub settings_hash: String,
}
