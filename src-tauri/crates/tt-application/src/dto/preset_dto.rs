use serde::{Deserialize, Serialize};
use serde_json::Value;
use tt_domain::models::preset::{Preset, PresetType};

/// DTO for saving a preset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavePresetDto {
    /// Name of the preset
    pub name: String,
    /// API ID (e.g., "openai", "kobold", "novel")
    #[serde(rename = "apiId")]
    pub api_id: String,
    /// Preset data as JSON
    pub preset: Value,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePresetResponseDto {
    /// Whether this is a default preset
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    /// Preset data
    pub preset: Value,
}

/// DTO for OpenAI preset save (specialized endpoint)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveOpenAIPresetDto {
    /// Preset data as JSON
    #[serde(flatten)]
    pub preset: Value,
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

impl TryFrom<SavePresetDto> for Preset {
    type Error = String;

    fn try_from(dto: SavePresetDto) -> Result<Self, Self::Error> {
        let preset_type = PresetType::from_api_id(&dto.api_id)
            .ok_or_else(|| format!("Unknown API ID: {}", dto.api_id))?;

        Ok(Preset::new(dto.name, preset_type, dto.preset))
    }
}

impl SavePresetResponseDto {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl RestorePresetResponseDto {
    pub fn new(is_default: bool, preset: Value) -> Self {
        Self { is_default, preset }
    }

    pub fn not_found() -> Self {
        Self {
            is_default: false,
            preset: Value::Object(serde_json::Map::new()),
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
