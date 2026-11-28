use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Avatar model representing a user avatar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Avatar {
    /// Name of the avatar file
    pub name: String,
    /// Path to the avatar file
    pub path: PathBuf,
}

/// Result of avatar upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarUploadResult {
    /// Path to the uploaded avatar
    pub path: String,
}
