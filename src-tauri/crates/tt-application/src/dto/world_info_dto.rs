use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorldInfoDto {
    pub name: String,
}

pub struct WorldInfoJsonDto {
    pub bytes: Vec<u8>,
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
