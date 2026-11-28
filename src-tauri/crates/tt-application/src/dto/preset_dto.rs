use serde::{Deserialize, Serialize};
use tt_domain::json_document::JsonDocument;

/// DTO for saving a preset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavePresetDto {
    /// Name of the preset
    pub name: String,
    /// API ID (e.g., "openai", "kobold", "novel")
    #[serde(rename = "apiId")]
    pub api_id: String,
    /// The preset document as JSON text
    pub preset_json: String,
}

/// DTO for deleting a preset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePresetDto {
    /// Name of the preset to delete
    pub name: String,
    /// API ID (e.g., "openai", "kobold", "novel")
    #[serde(rename = "apiId")]
    pub api_id: String,
}

/// DTO for restoring a default preset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePresetDto {
    /// Name of the preset to restore
    pub name: String,
    /// API ID (e.g., "openai", "kobold", "novel")
    #[serde(rename = "apiId")]
    pub api_id: String,
}

/// DTO for preset save response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavePresetResponseDto {
    /// Name of the saved preset
    pub name: String,
}

/// DTO for preset restore response
#[derive(Debug, Clone, Serialize)]
pub struct RestorePresetResponseDto {
    /// Whether this is a default preset
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    /// Preset data
    pub preset: JsonDocument,
}

/// DTO for OpenAI preset delete (specialized endpoint)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteOpenAIPresetDto {
    /// Name of the preset to delete
    pub name: String,
}

/// DTO for OpenAI preset delete response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteOpenAIPresetResponseDto {
    /// Whether the operation was successful
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    /// Whether there was an error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<bool>,
}

impl SavePresetResponseDto {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl RestorePresetResponseDto {
    pub fn new(is_default: bool, preset: JsonDocument) -> Self {
        Self { is_default, preset }
    }

    pub fn not_found() -> Self {
        Self {
            is_default: false,
            preset: JsonDocument::default(),
        }
    }
}

impl DeleteOpenAIPresetResponseDto {
    pub fn success() -> Self {
        Self {
            ok: Some(true),
            error: None,
        }
    }

    pub fn error() -> Self {
        Self {
            ok: None,
            error: Some(true),
        }
    }
}
