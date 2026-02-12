use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Represents different types of presets based on API source
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PresetType {
    /// Kobold AI presets (includes KoboldHorde)
    Kobold,
    /// Novel AI presets
    Novel,
    /// OpenAI presets
    OpenAI,
    /// Text Generation WebUI presets
    TextGen,
    /// Instruct templates
    Instruct,
    /// Context templates
    Context,
    /// System prompt templates
    SysPrompt,
    /// Reasoning templates
    Reasoning,
}

impl PresetType {
    /// Get the file extension for this preset type
    pub fn extension(&self) -> &'static str {
        ".json"
    }

    /// Get the directory name for this preset type
    pub fn directory_name(&self) -> &'static str {
        match self {
            PresetType::Kobold => "KoboldAI Settings",
            PresetType::Novel => "NovelAI Settings",
            PresetType::OpenAI => "OpenAI Settings",
            PresetType::TextGen => "TextGen Settings",
            PresetType::Instruct => "instruct",
            PresetType::Context => "context",
            PresetType::SysPrompt => "sysprompt",
            PresetType::Reasoning => "reasoning",
        }
    }

    /// Convert from API ID string to PresetType
    pub fn from_api_id(api_id: &str) -> Option<Self> {
        match api_id {
            "kobold" | "koboldhorde" => Some(PresetType::Kobold),
            "novel" => Some(PresetType::Novel),
            "openai" => Some(PresetType::OpenAI),
            "textgenerationwebui" => Some(PresetType::TextGen),
            "instruct" => Some(PresetType::Instruct),
            "context" => Some(PresetType::Context),
            "sysprompt" => Some(PresetType::SysPrompt),
            "reasoning" => Some(PresetType::Reasoning),
            _ => None,
        }
    }

    /// Convert to API ID string
    pub fn to_api_id(&self) -> &'static str {
        match self {
            PresetType::Kobold => "kobold",
            PresetType::Novel => "novel",
            PresetType::OpenAI => "openai",
            PresetType::TextGen => "textgenerationwebui",
            PresetType::Instruct => "instruct",
            PresetType::Context => "context",
            PresetType::SysPrompt => "sysprompt",
            PresetType::Reasoning => "reasoning",
        }
    }
}

impl fmt::Display for PresetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_api_id())
    }
}

/// Represents a preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Name of the preset
    pub name: String,
    /// Type of the preset (API source)
    pub preset_type: PresetType,
    /// Preset data as JSON value
    pub data: Value,
}

impl Preset {
    /// Create a new preset
    pub fn new(name: String, preset_type: PresetType, data: Value) -> Self {
        Self {
            name,
            preset_type,
            data,
        }
    }

    /// Validate the preset data
    pub fn validate(&self) -> Result<(), String> {
        // Basic validation - ensure data is an object
        if !self.data.is_object() {
            return Err("Preset data must be a JSON object".to_string());
        }

        // Ensure name is not empty
        if self.name.trim().is_empty() {
            return Err("Preset name cannot be empty".to_string());
        }

        Ok(())
    }

    /// Get the preset data with the name field included
    pub fn data_with_name(&self) -> Value {
        let mut data = self.data.clone();

        // Ensure data is an object
        if !data.is_object() {
            data = serde_json::json!({});
        }

        // Add the name to the data
        if let Some(obj) = data.as_object_mut() {
            obj.insert("name".to_string(), Value::String(self.name.clone()));
        }

        data
    }
}

/// Default preset information from content system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultPreset {
    /// Filename of the default preset
    pub filename: String,
    /// Name of the preset
    pub name: String,
    /// Type of the preset
    pub preset_type: PresetType,
    /// Whether this is a default preset
    pub is_default: bool,
    /// Preset data
    pub data: Value,
}

/// Sanitize a filename to be safe for filesystem use
pub fn sanitize_filename(name: &str) -> String {
    // Use a simple sanitization approach similar to the original
    name.chars()
        .map(|c| match c {
            // Replace unsafe characters with underscores
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            // Keep safe characters
            c if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' => c,
            // Replace other characters with underscores
            _ => '_',
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_preset_type_from_api_id() {
        assert_eq!(PresetType::from_api_id("kobold"), Some(PresetType::Kobold));
        assert_eq!(
            PresetType::from_api_id("koboldhorde"),
            Some(PresetType::Kobold)
        );
        assert_eq!(PresetType::from_api_id("novel"), Some(PresetType::Novel));
        assert_eq!(PresetType::from_api_id("openai"), Some(PresetType::OpenAI));
        assert_eq!(
            PresetType::from_api_id("textgenerationwebui"),
            Some(PresetType::TextGen)
        );
        assert_eq!(
            PresetType::from_api_id("instruct"),
            Some(PresetType::Instruct)
        );
        assert_eq!(
            PresetType::from_api_id("context"),
            Some(PresetType::Context)
        );
        assert_eq!(
            PresetType::from_api_id("sysprompt"),
            Some(PresetType::SysPrompt)
        );
        assert_eq!(
            PresetType::from_api_id("reasoning"),
            Some(PresetType::Reasoning)
        );
        assert_eq!(PresetType::from_api_id("unknown"), None);
    }

    #[test]
    fn test_preset_type_directory_name() {
        assert_eq!(PresetType::Kobold.directory_name(), "KoboldAI Settings");
        assert_eq!(PresetType::Novel.directory_name(), "NovelAI Settings");
        assert_eq!(PresetType::OpenAI.directory_name(), "OpenAI Settings");
        assert_eq!(PresetType::TextGen.directory_name(), "TextGen Settings");
        assert_eq!(PresetType::Instruct.directory_name(), "instruct");
        assert_eq!(PresetType::Context.directory_name(), "context");
        assert_eq!(PresetType::SysPrompt.directory_name(), "sysprompt");
        assert_eq!(PresetType::Reasoning.directory_name(), "reasoning");
    }

    #[test]
    fn test_preset_data_with_name() {
        let preset = Preset::new(
            "Test Preset".to_string(),
            PresetType::OpenAI,
            json!({"temperature": 0.7}),
        );

        let data_with_name = preset.data_with_name();
        assert_eq!(data_with_name["name"], "Test Preset");
        assert_eq!(data_with_name["temperature"], 0.7);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("normal_name"), "normal_name");
        assert_eq!(sanitize_filename("name with spaces"), "name with spaces");
        assert_eq!(
            sanitize_filename("name/with\\unsafe:chars"),
            "name_with_unsafe_chars"
        );
        assert_eq!(
            sanitize_filename("name*with?more\"unsafe<chars>"),
            "name_with_more_unsafe_chars_"
        );
    }

    #[test]
    fn test_preset_validation() {
        let valid_preset = Preset::new(
            "Valid Preset".to_string(),
            PresetType::OpenAI,
            json!({"temperature": 0.7}),
        );
        assert!(valid_preset.validate().is_ok());

        let invalid_preset = Preset::new(
            "".to_string(),
            PresetType::OpenAI,
            json!({"temperature": 0.7}),
        );
        assert!(invalid_preset.validate().is_err());

        let invalid_data_preset = Preset::new(
            "Invalid Data".to_string(),
            PresetType::OpenAI,
            Value::String("not an object".to_string()),
        );
        assert!(invalid_data_preset.validate().is_err());
    }
}
