use chrono::{DateTime, Utc};
use serde::de::{self};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// Character model representing a character card in SillyTavern format
/// Supports both V2 and V3 character card formats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    // Spec information
    #[serde(default = "default_spec")]
    pub spec: String,
    #[serde(default = "default_spec_version")]
    pub spec_version: String,

    // Core character information
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub first_mes: String,
    #[serde(default)]
    pub mes_example: String,

    // Avatar and chat information
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub chat: String,

    // Creator information
    #[serde(default)]
    pub creator: String,
    #[serde(default)]
    pub creator_notes: String,

    // Metadata
    #[serde(default)]
    pub character_version: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub create_date: String,

    // Extensions
    #[serde(default, deserialize_with = "deserialize_string_or_float")]
    pub talkativeness: f32,
    #[serde(default)]
    pub fav: bool,

    // V2 data structure
    #[serde(default)]
    pub data: CharacterData,

    // Internal fields (not part of the character card)
    #[serde(skip)]
    pub file_name: Option<String>,
    #[serde(skip)]
    pub chat_size: u64,
    #[serde(skip)]
    pub date_added: i64,
    #[serde(skip)]
    pub date_last_chat: i64,
    #[serde(skip)]
    pub json_data: Option<String>,
}

/// Character data structure for V2 character cards
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterData {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub first_mes: String,
    #[serde(default)]
    pub mes_example: String,

    #[serde(default)]
    pub creator_notes: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub post_history_instructions: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub creator: String,
    #[serde(default)]
    pub character_version: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub alternate_greetings: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub group_only_greetings: Vec<String>,

    #[serde(default)]
    pub extensions: CharacterExtensions,

    #[serde(default)]
    pub character_book: Option<serde_json::Value>,
}

/// Character extensions structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterExtensions {
    #[serde(default, deserialize_with = "deserialize_string_or_float")]
    pub talkativeness: f32,
    #[serde(default)]
    pub fav: bool,
    #[serde(default)]
    pub world: String,
    #[serde(default)]
    pub depth_prompt: DepthPrompt,
    #[serde(default, flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// Depth prompt structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DepthPrompt {
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_depth")]
    pub depth: i32,
    #[serde(default = "default_role")]
    pub role: String,
}

fn default_spec() -> String {
    "chara_card_v2".to_string()
}

fn default_spec_version() -> String {
    "2.0".to_string()
}

fn default_depth() -> i32 {
    4
}

fn default_role() -> String {
    "system".to_string()
}

/// Deserialize a value that can be either a string or a number into an f32
fn deserialize_string_or_float<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    // This will handle the deserialization
    struct StringOrFloat;

    impl<'de> de::Visitor<'de> for StringOrFloat {
        type Value = f32;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a float")
        }

        // Handle string values
        fn visit_str<E>(self, value: &str) -> Result<f32, E>
        where
            E: de::Error,
        {
            f32::from_str(value).map_err(|_| E::custom(format!("invalid float value: {}", value)))
        }

        // Handle float values
        fn visit_f32<E>(self, value: f32) -> Result<f32, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        // Handle float values as f64
        fn visit_f64<E>(self, value: f64) -> Result<f32, E>
        where
            E: de::Error,
        {
            Ok(value as f32)
        }

        // Handle integer values
        fn visit_i64<E>(self, value: i64) -> Result<f32, E>
        where
            E: de::Error,
        {
            Ok(value as f32)
        }

        // Handle unsigned integer values
        fn visit_u64<E>(self, value: u64) -> Result<f32, E>
        where
            E: de::Error,
        {
            Ok(value as f32)
        }
    }

    deserializer.deserialize_any(StringOrFloat)
}

/// Deserialize a string list that may be encoded as an array or comma-delimited string.
fn deserialize_string_or_array<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrArray;

    impl<'de> de::Visitor<'de> for StringOrArray {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string, string array, or null")
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Vec::new())
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Vec::new())
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect())
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                match value {
                    serde_json::Value::String(text) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            values.push(trimmed.to_string());
                        }
                    }
                    serde_json::Value::Number(number) => values.push(number.to_string()),
                    serde_json::Value::Bool(boolean) => values.push(boolean.to_string()),
                    _ => {}
                }
            }
            Ok(values)
        }
    }

    deserializer.deserialize_any(StringOrArray)
}

impl Character {
    /// Create a new character with basic information
    pub fn new(name: String, description: String, personality: String, first_mes: String) -> Self {
        let now = Utc::now();
        let timestamp = now.timestamp_millis();
        let formatted_date = humanized_date(now);
        let chat = format!("{} - {}", name, formatted_date);

        let character = Self {
            spec: default_spec(),
            spec_version: default_spec_version(),
            name: name.clone(),
            description: description.clone(),
            personality: personality.clone(),
            scenario: String::new(),
            first_mes: first_mes.clone(),
            mes_example: String::new(),
            avatar: "none".to_string(),
            chat: chat.clone(),
            creator: String::new(),
            creator_notes: String::new(),
            character_version: String::new(),
            tags: Vec::new(),
            create_date: formatted_date.clone(),
            talkativeness: 0.5,
            fav: false,
            data: CharacterData {
                name: name.clone(),
                description: description.clone(),
                personality: personality.clone(),
                first_mes: first_mes.clone(),
                extensions: CharacterExtensions {
                    talkativeness: 0.5,
                    fav: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            file_name: None,
            chat_size: 0,
            date_added: timestamp,
            date_last_chat: 0,
            json_data: None,
        };

        character
    }

    /// Convert character to V2 format
    pub fn to_v2(&self) -> Self {
        let mut character = self.clone();
        character.spec = "chara_card_v2".to_string();
        character.spec_version = "2.0".to_string();

        // Ensure data fields are synchronized with top-level fields
        character.data.name = character.name.clone();
        character.data.description = character.description.clone();
        character.data.personality = character.personality.clone();
        character.data.scenario = character.scenario.clone();
        character.data.first_mes = character.first_mes.clone();
        character.data.mes_example = character.mes_example.clone();
        character.data.creator_notes = character.creator_notes.clone();
        character.data.creator = character.creator.clone();
        character.data.character_version = character.character_version.clone();
        character.data.tags = character.tags.clone();
        character.data.extensions.talkativeness = character.talkativeness;
        character.data.extensions.fav = character.fav;

        character
    }

    /// Convert character to V3 format
    pub fn to_v3(&self) -> Self {
        let mut character = self.to_v2();
        character.spec = "chara_card_v3".to_string();
        character.spec_version = "3.0".to_string();

        character
    }

    /// Get the file name for this character
    pub fn get_file_name(&self) -> String {
        if let Some(file_name) = &self.file_name {
            file_name.clone()
        } else {
            sanitize_filename(&self.name)
        }
    }
}

/// Sanitize a filename to be safe for file systems
pub fn sanitize_filename(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>();

    sanitized.trim().to_string()
}

/// Format a date in a human-readable format
pub fn humanized_date(date: DateTime<Utc>) -> String {
    date.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}
