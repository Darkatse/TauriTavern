use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::models::chat::humanized_date as humanized_chat_date;
use crate::models::filename::sanitize_filename;

/// Backend projection of a SillyTavern character card.
///
/// The stored JSON remains authoritative; this type contains only values the
/// application needs to operate on.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Character {
    // Spec information
    pub spec: String,
    pub spec_version: String,

    // Core character information
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub first_mes: String,
    pub mes_example: String,

    // Avatar and chat information
    pub avatar: String,
    pub chat: String,

    // Creator information
    pub creator: String,
    pub creator_notes: String,

    // Metadata
    pub character_version: String,
    pub tags: Vec<String>,
    pub create_date: String,

    // Extensions
    pub talkativeness: f64,
    pub fav: bool,

    // V2 data structure
    pub data: CharacterData,

    // Internal fields (not part of the character card)
    #[serde(skip)]
    pub file_name: Option<String>,
    #[serde(skip)]
    pub chat_size: u64,
    #[serde(skip)]
    pub data_size: u64,
    #[serde(skip)]
    pub date_added: i64,
    #[serde(skip)]
    pub date_last_chat: i64,
    #[serde(skip)]
    pub json_data: Option<String>,
    #[serde(skip)]
    pub shallow: bool,
}

/// Backend projection of the V2/V3 `data` object.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterData {
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub first_mes: String,
    pub mes_example: String,

    pub creator_notes: String,
    pub system_prompt: String,
    pub post_history_instructions: String,
    pub tags: Vec<String>,
    pub creator: String,
    pub character_version: String,
    pub alternate_greetings: Vec<String>,
    pub group_only_greetings: Vec<String>,

    pub extensions: CharacterExtensions,

    pub character_book: Option<serde_json::Value>,
}

/// Open SillyTavern/third-party extension object.
///
/// The character-card specs deliberately leave this object open. Rust reads
/// owned values through accessors and only changes them through explicit
/// setters; every other value keeps its original JSON representation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct CharacterExtensions {
    values: Map<String, Value>,
}

impl CharacterExtensions {
    fn from_card_value(value: Option<&Value>) -> Self {
        Self {
            values: value
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.values.insert(key.into(), value);
    }

    pub fn talkativeness(&self) -> f64 {
        projected_number(self.get("talkativeness")).unwrap_or(0.0)
    }

    pub fn set_talkativeness(&mut self, value: f64) {
        self.insert("talkativeness", json!(value));
    }

    pub fn fav(&self) -> bool {
        projected_bool(self.get("fav"))
    }

    pub fn set_fav(&mut self, value: bool) {
        self.insert("fav", Value::Bool(value));
    }

    pub fn world(&self) -> &str {
        self.get("world").and_then(Value::as_str).unwrap_or("")
    }

    pub fn set_world(&mut self, value: impl Into<String>) {
        self.insert("world", Value::String(value.into()));
    }
}

fn default_spec() -> String {
    "chara_card_v2".to_string()
}

fn default_spec_version() -> String {
    "2.0".to_string()
}

fn projected_string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

fn projected_string_list(value: Option<&Value>, split_string: bool) -> Vec<String> {
    match value {
        Some(Value::String(value)) if split_string => value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        Some(Value::String(value)) => {
            let value = value.trim();
            (!value.is_empty())
                .then(|| value.to_string())
                .into_iter()
                .collect()
        }
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                Value::String(value) => Some(value.trim().to_string()),
                Value::Number(value) => Some(value.to_string()),
                Value::Bool(value) => Some(value.to_string()),
                _ => None,
            })
            .filter(|value| !value.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn projected_number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(value) => value.as_f64(),
        Value::String(value) if value.trim().is_empty() => Some(0.0),
        Value::String(value) => value.trim().parse().ok(),
        Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Value::Null => Some(0.0),
        _ => None,
    }
}

fn projected_bool(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
        Some(Value::Null) | None => false,
    }
}

impl CharacterData {
    fn from_card_value(value: Option<&Value>) -> Self {
        let object = value.and_then(Value::as_object);
        let field = |name| object.and_then(|object| object.get(name));

        Self {
            name: projected_string(field("name")),
            description: projected_string(field("description")),
            personality: projected_string(field("personality")),
            scenario: projected_string(field("scenario")),
            first_mes: projected_string(field("first_mes")),
            mes_example: projected_string(field("mes_example")),
            creator_notes: projected_string(field("creator_notes")),
            system_prompt: projected_string(field("system_prompt")),
            post_history_instructions: projected_string(field("post_history_instructions")),
            tags: projected_string_list(field("tags"), true),
            creator: projected_string(field("creator")),
            character_version: projected_string(field("character_version")),
            alternate_greetings: projected_string_list(field("alternate_greetings"), false),
            group_only_greetings: projected_string_list(field("group_only_greetings"), false),
            extensions: CharacterExtensions::from_card_value(field("extensions")),
            character_book: field("character_book")
                .filter(|value| !value.is_null())
                .cloned(),
        }
    }
}

impl Character {
    /// Build the internal projection of an open character-card document.
    ///
    /// External cards must enter through this function rather than Serde's
    /// struct decoder: fields irrelevant to the current Rust use case remain
    /// raw JSON and cannot make the whole card unreadable.
    pub fn from_card_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let field = |name| object.get(name);

        Some(Self {
            spec: field("spec")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .unwrap_or_else(default_spec),
            spec_version: field("spec_version")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .unwrap_or_else(default_spec_version),
            name: projected_string(field("name")),
            description: projected_string(field("description")),
            personality: projected_string(field("personality")),
            scenario: projected_string(field("scenario")),
            first_mes: projected_string(field("first_mes")),
            mes_example: projected_string(field("mes_example")),
            avatar: projected_string(field("avatar")),
            chat: projected_string(field("chat")),
            creator: projected_string(field("creator")),
            creator_notes: projected_string(field("creator_notes")),
            character_version: projected_string(field("character_version")),
            tags: projected_string_list(field("tags"), true),
            create_date: projected_string(field("create_date")),
            talkativeness: projected_number(field("talkativeness")).unwrap_or(0.0),
            fav: projected_bool(field("fav")),
            data: CharacterData::from_card_value(field("data")),
            ..Default::default()
        })
    }

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
                extensions: {
                    let mut extensions = CharacterExtensions::default();
                    extensions.set_talkativeness(0.5);
                    extensions.set_fav(false);
                    extensions.set_world("");
                    extensions.insert(
                        "depth_prompt",
                        json!({ "prompt": "", "depth": 4, "role": "system" }),
                    );
                    extensions
                },
                ..Default::default()
            },
            file_name: None,
            chat_size: 0,
            data_size: 0,
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
        self.data.extensions.set_talkativeness(self.talkativeness);
        self.data.extensions.set_fav(self.fav);
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
            self.talkativeness = self.data.extensions.talkativeness();
        }

        self.fav = self.fav || self.data.extensions.fav();

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

        let world = self.data.extensions.get("world").cloned();
        self.data.extensions = CharacterExtensions::default();
        self.data.extensions.set_talkativeness(self.talkativeness);
        self.data.extensions.set_fav(self.fav);
        if let Some(world) = world {
            self.data.extensions.insert("world", world);
        }

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
        character.data.extensions.insert(
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
        assert!(shallow.data.extensions.get("regex_scripts").is_none());
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
        character.data.extensions.set_talkativeness(0.8);

        let value = serde_json::to_value(character.to_v2()).expect("serialize character");

        assert_eq!(value.get("talkativeness"), Some(&Value::from(0.8)));
        assert_eq!(
            value.pointer("/data/extensions/talkativeness"),
            Some(&Value::from(0.8))
        );
    }
}
