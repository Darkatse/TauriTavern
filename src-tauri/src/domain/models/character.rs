use chrono::{SecondsFormat, Utc};
use serde::de::{self};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::models::chat::humanized_date as humanized_chat_date;

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
    #[serde(skip)]
    pub shallow: bool,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthPrompt {
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_depth")]
    pub depth: i32,
    #[serde(default = "default_role")]
    pub role: String,
}

impl Default for DepthPrompt {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            depth: default_depth(),
            role: default_role(),
        }
    }
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
        let create_date = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let chat = format!("{} - {}", name, humanized_chat_date(now));

        Self {
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
            create_date,
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
            shallow: false,
        }
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

    /// Get the file name for this character
    pub fn get_file_name(&self) -> String {
        if let Some(file_name) = &self.file_name {
            file_name.clone()
        } else {
            sanitize_filename(&self.name)
        }
    }

    /// Build a shallow projection for character list rendering.
    pub fn into_shallow(mut self) -> Self {
        fn pick_non_empty(primary: &str, fallback: &str) -> String {
            if primary.trim().is_empty() {
                fallback.to_string()
            } else {
                primary.to_string()
            }
        }

        // Keep only fields required by upstream-compatible character list rendering.
        // The full card will be fetched via `/api/characters/get` when needed.
        self.name = pick_non_empty(&self.name, &self.data.name);
        self.creator = pick_non_empty(&self.creator, &self.data.creator);
        self.creator_notes = pick_non_empty(&self.creator_notes, &self.data.creator_notes);
        self.character_version = pick_non_empty(&self.character_version, &self.data.character_version);

        if self.tags.is_empty() {
            self.tags = self.data.tags.clone();
        }

        if self.talkativeness == 0.0 {
            self.talkativeness = self.data.extensions.talkativeness;
        }

        self.fav = self.fav || self.data.extensions.fav;

        // Drop heavy card payload from shallow projection.
        self.description.clear();
        self.personality.clear();
        self.scenario.clear();
        self.first_mes.clear();
        self.mes_example.clear();

        self.data.name = self.name.clone();
        self.data.description.clear();
        self.data.personality.clear();
        self.data.scenario.clear();
        self.data.first_mes.clear();
        self.data.mes_example.clear();
        self.data.creator = self.creator.clone();
        self.data.creator_notes = self.creator_notes.clone();
        self.data.character_version = self.character_version.clone();
        self.data.tags = self.tags.clone();

        self.data.system_prompt.clear();
        self.data.post_history_instructions.clear();
        self.data.alternate_greetings.clear();
        self.data.group_only_greetings.clear();

        self.data.extensions.talkativeness = self.talkativeness;
        self.data.extensions.fav = self.fav;
        self.data.extensions.world.clear();
        self.data.extensions.depth_prompt = DepthPrompt::default();
        self.data.extensions.additional.clear();

        self.data.character_book = None;
        self.json_data = None;
        self.shallow = true;

        self
    }
}

#[cfg(test)]
mod tests {
    use super::Character;

    #[test]
    fn into_shallow_drops_heavy_character_payload() {
        let mut character = Character::new(
            "Alice".to_string(),
            "A very long description".to_string(),
            "A personality".to_string(),
            "Hello!".to_string(),
        );

        character.data.system_prompt = "system prompt".to_string();
        character.data.post_history_instructions = "jailbreak".to_string();
        character.data.alternate_greetings = vec!["hi".to_string()];
        character.data.group_only_greetings = vec!["group-hi".to_string()];
        character.data.character_book = Some(serde_json::json!({ "entries": { "1": {} } }));
        character
            .data
            .extensions
            .additional
            .insert(
                "regex_scripts".to_string(),
                serde_json::json!([{ "replaceString": "x".repeat(1024) }]),
            );
        character.json_data = Some("{\"huge\":true}".to_string());

        let shallow = character.into_shallow();

        assert!(shallow.shallow);
        assert_eq!(shallow.name, "Alice");
        assert_eq!(shallow.data.name, "Alice");

        assert!(shallow.description.is_empty());
        assert!(shallow.personality.is_empty());
        assert!(shallow.first_mes.is_empty());
        assert!(shallow.data.system_prompt.is_empty());
        assert!(shallow.data.post_history_instructions.is_empty());
        assert!(shallow.data.alternate_greetings.is_empty());
        assert!(shallow.data.group_only_greetings.is_empty());
        assert!(shallow.data.extensions.additional.is_empty());
        assert!(shallow.data.character_book.is_none());
        assert!(shallow.json_data.is_none());
    }
}

/// Sanitize a filename to be safe for file systems
pub fn sanitize_filename(name: &str) -> String {
    const MAX_FILENAME_BYTES: usize = 255;

    fn is_illegal_character(ch: char) -> bool {
        matches!(ch, '/' | '?' | '<' | '>' | '\\' | ':' | '*' | '|' | '"')
    }

    fn is_control_code(ch: char) -> bool {
        let value = ch as u32;
        (0x00..=0x1F).contains(&value) || (0x80..=0x9F).contains(&value)
    }

    fn is_reserved_dots_only(value: &str) -> bool {
        !value.is_empty() && value.chars().all(|ch| ch == '.')
    }

    fn is_windows_reserved_name(value: &str) -> bool {
        if value.is_empty() {
            return false;
        }

        let lower = value.to_ascii_lowercase();
        let stem = lower.split('.').next().unwrap_or(lower.as_str());

        matches!(stem, "con" | "prn" | "aux" | "nul")
            || stem
                .strip_prefix("com")
                .is_some_and(|suffix| suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit())
            || stem
                .strip_prefix("lpt")
                .is_some_and(|suffix| suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit())
    }

    fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> &str {
        if value.len() <= max_bytes {
            return value;
        }

        let mut end = 0usize;
        for (index, ch) in value.char_indices() {
            let next = index + ch.len_utf8();
            if next > max_bytes {
                break;
            }
            end = next;
        }

        &value[..end]
    }

    let mut sanitized = String::with_capacity(name.len());
    for ch in name.chars() {
        if is_illegal_character(ch) || is_control_code(ch) {
            continue;
        }

        sanitized.push(ch);
    }

    if is_reserved_dots_only(&sanitized) || is_windows_reserved_name(&sanitized) {
        sanitized.clear();
    }

    while sanitized.ends_with('.') || sanitized.ends_with(' ') {
        sanitized.pop();
    }

    let trimmed = sanitized.trim();
    truncate_utf8_bytes(trimmed, MAX_FILENAME_BYTES).to_string()
}

#[cfg(test)]
mod filename_tests {
    use super::sanitize_filename;

    #[test]
    fn sanitize_filename_removes_illegal_characters() {
        assert_eq!(sanitize_filename("a:b*c?.png"), "abc.png");
        assert_eq!(sanitize_filename("中文/测试"), "中文测试");
    }

    #[test]
    fn sanitize_filename_removes_reserved_names() {
        assert_eq!(sanitize_filename("."), "");
        assert_eq!(sanitize_filename(".."), "");
        assert_eq!(sanitize_filename("CON"), "");
        assert_eq!(sanitize_filename("com1.txt"), "");
    }

    #[test]
    fn sanitize_filename_strips_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("name. "), "name");
        assert_eq!(sanitize_filename("name..."), "name");
    }

    #[test]
    fn sanitize_filename_truncates_by_utf8_bytes() {
        let long_ascii = "a".repeat(300);
        let sanitized = sanitize_filename(&long_ascii);
        assert_eq!(sanitized.len(), 255);

        let long_cjk = "中".repeat(200);
        let sanitized = sanitize_filename(&long_cjk);
        assert_eq!(sanitized, "中".repeat(85));
        assert_eq!(sanitized.as_bytes().len(), 255);
    }
}
