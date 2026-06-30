use chrono::{SecondsFormat, Utc};
use serde::de::{self};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use crate::models::chat::humanized_date as humanized_chat_date;
use crate::models::filename::sanitize_filename;

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
    pub talkativeness: f64,
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
    pub talkativeness: f64,
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

/// Deserialize a value that can be either a string or a number into an f64.
fn deserialize_string_or_float<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrFloat;

    impl<'de> de::Visitor<'de> for StringOrFloat {
        type Value = f64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a float")
        }

        fn visit_str<E>(self, value: &str) -> Result<f64, E>
        where
            E: de::Error,
        {
            f64::from_str(value).map_err(|_| E::custom(format!("invalid float value: {}", value)))
        }

        fn visit_f32<E>(self, value: f32) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(f64::from(value))
        }

        fn visit_f64<E>(self, value: f64) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(value as f64)
        }

        fn visit_u64<E>(self, value: u64) -> Result<f64, E>
        where
            E: de::Error,
        {
            Ok(value as f64)
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
        character.sync_top_level_fields_to_v2_data();

        character
    }

    /// Synchronize legacy top-level fields into the V2 `data` object before persisting.
    pub(crate) fn sync_top_level_fields_to_v2_data(&mut self) {
        self.data.name = self.name.clone();
        self.data.description = self.description.clone();
        self.data.personality = self.personality.clone();
        self.data.scenario = self.scenario.clone();
        self.data.first_mes = self.first_mes.clone();
        self.data.mes_example = self.mes_example.clone();
        self.data.creator_notes = self.creator_notes.clone();
        self.data.creator = self.creator.clone();
        self.data.character_version = self.character_version.clone();
        self.data.tags = self.tags.clone();
        self.data.extensions.talkativeness = self.talkativeness;
        self.data.extensions.fav = self.fav;
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
        self.character_version =
            pick_non_empty(&self.character_version, &self.data.character_version);

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
    use serde_json::Value;

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
        character.data.extensions.additional.insert(
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

    #[test]
    fn talkativeness_serializes_as_clean_json_number() {
        let mut character = Character::new(
            "Alice".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hello".to_string(),
        );
        character.talkativeness = 0.8;
        character.data.extensions.talkativeness = 0.8;

        let value = serde_json::to_value(character.to_v2()).expect("serialize character");

        assert_eq!(value.get("talkativeness"), Some(&Value::from(0.8)));
        assert_eq!(
            value.pointer("/data/extensions/talkativeness"),
            Some(&Value::from(0.8))
        );
    }
}
