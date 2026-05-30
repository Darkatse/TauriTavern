use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_animated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_color: Option<String>,
    #[serde(default)]
    pub folder_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_timestamp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_resolution: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<f64>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Default for ImageMetadata {
    fn default() -> Self {
        Self {
            hash: None,
            aspect_ratio: None,
            is_animated: None,
            dominant_color: None,
            folder_ids: Vec::new(),
            added_timestamp: None,
            thumbnail_resolution: None,
            mtime: None,
            extra: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadataFolder {
    pub id: String,
    pub name: String,
    pub thumbnail_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageMetadataIndex {
    pub version: u8,
    #[serde(default)]
    pub images: HashMap<String, ImageMetadata>,
    #[serde(default)]
    pub folders: Vec<ImageMetadataFolder>,
}

impl Default for ImageMetadataIndex {
    fn default() -> Self {
        Self {
            version: 1,
            images: HashMap::new(),
            folders: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundFoldersPayload {
    pub folders: Vec<ImageMetadataFolder>,
    pub image_folder_map: HashMap<String, Vec<String>>,
}
