use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::domain::models::preset::{Preset, PresetType, DefaultPreset};

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

/// DTO for preset information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetDto {
    /// Name of the preset
    pub name: String,
    /// Type of the preset
    pub preset_type: String,
    /// Preset data
    pub data: Value,
}

/// DTO for default preset information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultPresetDto {
    /// Filename of the default preset
    pub filename: String,
    /// Name of the preset
    pub name: String,
    /// Type of the preset
    pub preset_type: String,
    /// Whether this is a default preset
    pub is_default: bool,
    /// Preset data
    pub data: Value,
}

// Conversion implementations

impl From<Preset> for PresetDto {
    fn from(preset: Preset) -> Self {
        Self {
            name: preset.name,
            preset_type: preset.preset_type.to_api_id().to_string(),
            data: preset.data,
        }
    }
}

impl From<DefaultPreset> for DefaultPresetDto {
    fn from(default_preset: DefaultPreset) -> Self {
        Self {
            filename: default_preset.filename,
            name: default_preset.name,
            preset_type: default_preset.preset_type.to_api_id().to_string(),
            is_default: default_preset.is_default,
            data: default_preset.data,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_save_preset_dto_conversion() {
        let dto = SavePresetDto {
            name: "Test Preset".to_string(),
            api_id: "openai".to_string(),
            preset: json!({"temperature": 0.7}),
        };

        let preset: Result<Preset, String> = dto.try_into();
        assert!(preset.is_ok());

        let preset = preset.unwrap();
        assert_eq!(preset.name, "Test Preset");
        assert_eq!(preset.preset_type, PresetType::OpenAI);
        assert_eq!(preset.data["temperature"], 0.7);
    }

    #[test]
    fn test_preset_dto_from_preset() {
        let preset = Preset::new(
            "Test Preset".to_string(),
            PresetType::OpenAI,
            json!({"temperature": 0.7}),
        );

        let dto: PresetDto = preset.into();
        assert_eq!(dto.name, "Test Preset");
        assert_eq!(dto.preset_type, "openai");
        assert_eq!(dto.data["temperature"], 0.7);
    }

    #[test]
    fn test_restore_preset_response_dto() {
        let response = RestorePresetResponseDto::new(true, json!({"temperature": 0.7}));
        assert!(response.is_default);
        assert_eq!(response.preset["temperature"], 0.7);

        let not_found = RestorePresetResponseDto::not_found();
        assert!(!not_found.is_default);
        assert!(not_found.preset.is_object());
    }

    #[test]
    fn test_delete_openai_preset_response_dto() {
        let success = DeleteOpenAIPresetResponseDto::success();
        assert_eq!(success.ok, Some(true));
        assert_eq!(success.error, None);

        let error = DeleteOpenAIPresetResponseDto::error();
        assert_eq!(error.ok, None);
        assert_eq!(error.error, Some(true));
    }
}
