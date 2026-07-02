use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorldInfoDto {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorldInfosBatchDto {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorldInfosBatchItemDto {
    pub name: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorldInfosBatchResponseDto {
    pub items: Vec<GetWorldInfosBatchItemDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizeWorldInfoNameDto {
    pub name: String,
    #[serde(default)]
    pub import_filename: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizeWorldInfoNameResponseDto {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveWorldInfoDto {
    pub name: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteWorldInfoDto {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportWorldInfoDto {
    pub file_path: String,
    pub original_filename: String,
    #[serde(default)]
    pub converted_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportWorldInfoResponseDto {
    pub name: String,
}
