use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateImageMetadataFolderDto {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateImageMetadataFolderDto {
    pub id: String,
    pub name: Option<String>,
    pub thumbnail_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteImageMetadataFolderDto {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadataFolderThumbnailUpdateDto {
    pub id: String,
    pub thumbnail_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetImageMetadataFolderThumbnailsDto {
    pub updates: Vec<ImageMetadataFolderThumbnailUpdateDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadataFolderAssignmentDto {
    pub id: String,
    pub paths: Vec<String>,
}
