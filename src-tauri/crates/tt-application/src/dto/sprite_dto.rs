use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ListSpritesDto {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadSpriteDto {
    pub name: String,
    pub sprite_name: String,
    pub original_filename: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadSpritePackDto {
    pub name: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteSpriteDto {
    pub name: String,
    pub sprite_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SpriteDto {
    pub label: String,
    pub path: String,
}
