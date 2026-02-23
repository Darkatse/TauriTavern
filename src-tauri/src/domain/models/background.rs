use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Represents a background image in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Background {
    /// The filename of the background image
    pub filename: String,

    /// The path to the background image
    pub path: String,
}

impl Background {
    /// Create a new Background instance
    pub fn new(filename: String, path: String) -> Self {
        Self { filename, path }
    }
}

#[derive(Debug, Clone)]
pub struct BackgroundAsset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundImageMetadata {
    pub aspect_ratio: f64,
    pub is_animated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_color: Option<String>,
    pub added_timestamp: i64,
    pub thumbnail_resolution: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundImageMetadataIndex {
    pub version: u8,
    pub images: HashMap<String, BackgroundImageMetadata>,
}
