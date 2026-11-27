use serde::{Deserialize, Serialize};
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
