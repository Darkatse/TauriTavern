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

/// Crop information for avatar image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropInfo {
    /// X coordinate of the crop
    pub x: i32,
    /// Y coordinate of the crop
    pub y: i32,
    /// Width of the crop
    pub width: i32,
    /// Height of the crop
    pub height: i32,
}

/// Result of avatar upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarUploadResult {
    /// Path to the uploaded avatar
    pub path: String,
}
