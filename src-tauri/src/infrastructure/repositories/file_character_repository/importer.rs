use chrono::Utc;
use serde_json::Value;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::character::{sanitize_filename, Character};
use crate::domain::models::chat::humanized_date as humanized_chat_date;
use crate::infrastructure::persistence::png_utils::{
    read_character_data_from_png, write_character_data_to_png,
};

use super::FileCharacterRepository;

impl FileCharacterRepository {
    fn parse_hex_escape(digits: &[u8]) -> Option<u16> {
        if digits.len() != 4 {
            return None;
        }

        let mut value = 0u16;
        for digit in digits {
            let nibble = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return None,
            };
            value = (value << 4) | nibble as u16;
        }

        Some(value)
    }

    fn normalize_json_surrogate_escapes(json_data: &str) -> Cow<'_, str> {
        let bytes = json_data.as_bytes();
        let mut output: Option<String> = None;
        let mut copy_start = 0usize;
        let mut index = 0usize;

        while index < bytes.len() {
            if index + 6 <= bytes.len()
                && bytes[index] == b'\\'
                && bytes[index + 1] == b'u'
                && Self::is_unescaped_backslash(bytes, index)
            {
                if let Some(code) = Self::parse_hex_escape(&bytes[index + 2..index + 6]) {
                    let is_high_surrogate = (0xD800..=0xDBFF).contains(&code);
                    let is_low_surrogate = (0xDC00..=0xDFFF).contains(&code);

                    if is_high_surrogate {
                        let has_valid_low_pair = if index + 12 <= bytes.len()
                            && bytes[index + 6] == b'\\'
                            && bytes[index + 7] == b'u'
                        {
                            match Self::parse_hex_escape(&bytes[index + 8..index + 12]) {
                                Some(next) => (0xDC00..=0xDFFF).contains(&next),
                                None => false,
                            }
                        } else {
                            false
                        };

                        if has_valid_low_pair {
                            index += 12;
                            continue;
                        }

                        let out =
                            output.get_or_insert_with(|| String::with_capacity(json_data.len()));
                        out.push_str(&json_data[copy_start..index]);
                        out.push_str("\\uFFFD");
                        index += 6;
                        copy_start = index;
                        continue;
                    }

                    if is_low_surrogate {
                        let out =
                            output.get_or_insert_with(|| String::with_capacity(json_data.len()));
                        out.push_str(&json_data[copy_start..index]);
                        out.push_str("\\uFFFD");
                        index += 6;
                        copy_start = index;
                        continue;
                    }
                }
            }

            index += 1;
        }

        if let Some(mut out) = output {
            out.push_str(&json_data[copy_start..]);
            Cow::Owned(out)
        } else {
            Cow::Borrowed(json_data)
        }
    }

    fn is_unescaped_backslash(bytes: &[u8], index: usize) -> bool {
        let mut preceding = 0usize;
        let mut cursor = index;

        while cursor > 0 {
            cursor -= 1;
            if bytes[cursor] != b'\\' {
                break;
            }
            preceding += 1;
        }

        preceding % 2 == 0
    }

    fn parse_alternate_greetings(value: Option<&Value>) -> Vec<String> {
        match value {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect(),
            Some(Value::String(item)) => {
                let trimmed = item.trim();
                if trimmed.is_empty() {
                    Vec::new()
                } else {
                    vec![trimmed.to_string()]
                }
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn parse_imported_character_json(
        &self,
        json_data: &str,
    ) -> Result<Character, DomainError> {
        let normalized_json = Self::normalize_json_surrogate_escapes(json_data);

        let raw_value: Value = serde_json::from_str(&normalized_json).map_err(|e| {
            DomainError::InvalidData(format!("Failed to parse character JSON: {}", e))
        })?;
        let has_talkativeness = raw_value.get("talkativeness").is_some()
            || raw_value
                .pointer("/data/extensions/talkativeness")
                .is_some();

        let mut character: Character = serde_json::from_value(raw_value.clone()).map_err(|e| {
            DomainError::InvalidData(format!("Failed to decode character payload: {}", e))
        })?;

        self.apply_legacy_aliases(&mut character, &raw_value);
        self.normalize_imported_character(&mut character)?;
        if !has_talkativeness
            && character.talkativeness == 0.0
            && character.data.extensions.talkativeness == 0.0
        {
            character.talkativeness = 0.5;
            character.data.extensions.talkativeness = 0.5;
        }

        Ok(character)
    }

    pub(crate) fn apply_legacy_aliases(&self, character: &mut Character, raw_value: &Value) {
        if character.creator_notes.trim().is_empty() {
            if let Some(value) = raw_value.get("creatorcomment").and_then(Value::as_str) {
                character.creator_notes = value.to_string();
            }
        }

        if character.name.trim().is_empty() {
            if let Some(value) = raw_value.get("char_name").and_then(Value::as_str) {
                character.name = value.to_string();
            }
        }

        if character.description.trim().is_empty() {
            if let Some(value) = raw_value.get("char_persona").and_then(Value::as_str) {
                character.description = value.to_string();
            }
        }

        if character.first_mes.trim().is_empty() {
            if let Some(value) = raw_value.get("char_greeting").and_then(Value::as_str) {
                character.first_mes = value.to_string();
            }
        }

        if character.mes_example.trim().is_empty() {
            if let Some(value) = raw_value.get("example_dialogue").and_then(Value::as_str) {
                character.mes_example = value.to_string();
            }
        }

        if character.scenario.trim().is_empty() {
            if let Some(value) = raw_value.get("world_scenario").and_then(Value::as_str) {
                character.scenario = value.to_string();
            }
        }

        if character.tags.is_empty() {
            if let Some(array) = raw_value.get("tags").and_then(Value::as_array) {
                character.tags = array
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|tag| tag.trim().to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            } else if let Some(csv) = raw_value.get("tags").and_then(Value::as_str) {
                character.tags = csv
                    .split(',')
                    .map(|tag| tag.trim().to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            }
        }

        if character.data.alternate_greetings.is_empty() {
            character.data.alternate_greetings = Self::parse_alternate_greetings(
                raw_value
                    .get("alternate_greetings")
                    .or_else(|| raw_value.pointer("/data/alternate_greetings")),
            );
        }
    }

    fn sync_string_field(primary: &mut String, secondary: &mut String) {
        if primary.trim().is_empty() && !secondary.trim().is_empty() {
            *primary = secondary.clone();
        } else if secondary.trim().is_empty() && !primary.trim().is_empty() {
            *secondary = primary.clone();
        }
    }

    pub(crate) fn normalize_imported_character(
        &self,
        character: &mut Character,
    ) -> Result<(), DomainError> {
        Self::sync_string_field(&mut character.name, &mut character.data.name);
        character.name = character.name.trim().to_string();
        if character.name.is_empty() {
            return Err(DomainError::InvalidData(
                "Character name is missing".to_string(),
            ));
        }
        character.data.name = character.name.clone();

        Self::sync_string_field(&mut character.description, &mut character.data.description);
        Self::sync_string_field(&mut character.personality, &mut character.data.personality);
        Self::sync_string_field(&mut character.scenario, &mut character.data.scenario);
        Self::sync_string_field(&mut character.first_mes, &mut character.data.first_mes);
        Self::sync_string_field(&mut character.mes_example, &mut character.data.mes_example);
        Self::sync_string_field(&mut character.creator, &mut character.data.creator);
        Self::sync_string_field(
            &mut character.creator_notes,
            &mut character.data.creator_notes,
        );
        Self::sync_string_field(
            &mut character.character_version,
            &mut character.data.character_version,
        );

        if character.tags.is_empty() && !character.data.tags.is_empty() {
            character.tags = character.data.tags.clone();
        } else if character.data.tags.is_empty() && !character.tags.is_empty() {
            character.data.tags = character.tags.clone();
        }

        let top_talkativeness = character.talkativeness;
        let data_talkativeness = character.data.extensions.talkativeness;
        if top_talkativeness == 0.0 && data_talkativeness != 0.0 {
            character.talkativeness = data_talkativeness;
        } else if data_talkativeness == 0.0 {
            character.data.extensions.talkativeness = top_talkativeness;
        }

        let fav = character.fav || character.data.extensions.fav;
        character.fav = fav;
        character.data.extensions.fav = fav;

        if character.spec.trim().is_empty() {
            character.spec = "chara_card_v2".to_string();
        }
        if character.spec_version.trim().is_empty() {
            character.spec_version = "2.0".to_string();
        }

        character.chat = Self::normalize_chat_file_stem(&character.chat, &character.name);

        if character.create_date.trim().is_empty() {
            character.create_date = Utc::now().to_rfc3339();
        }

        Ok(())
    }

    fn normalize_preserved_file_stem(raw_name: &str) -> Result<String, DomainError> {
        let trimmed = raw_name.trim();
        if trimmed.is_empty() {
            return Err(DomainError::InvalidData(
                "Preserved file name is empty".to_string(),
            ));
        }

        let stem = Path::new(trimmed)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(trimmed);
        let normalized = sanitize_filename(stem);

        if normalized.is_empty() {
            return Err(DomainError::InvalidData(
                "Preserved file name is invalid".to_string(),
            ));
        }

        Ok(normalized)
    }

    fn resolve_import_file_stem(
        &self,
        character: &Character,
        source_path: &Path,
        preserve_file_name: Option<&str>,
    ) -> Result<String, DomainError> {
        if let Some(name) = preserve_file_name {
            let name = name.trim();
            if !name.is_empty() {
                return Self::normalize_preserved_file_stem(name);
            }
        }

        let mut base = sanitize_filename(&character.name);
        if base.is_empty() {
            base = source_path
                .file_stem()
                .and_then(|value| value.to_str())
                .map(sanitize_filename)
                .unwrap_or_default();
        }

        if base.is_empty() {
            return Err(DomainError::InvalidData(
                "Unable to determine character file name".to_string(),
            ));
        }

        Ok(self.ensure_unique_file_stem(&base))
    }

    fn ensure_unique_file_stem(&self, base: &str) -> String {
        let mut candidate = base.to_string();
        let mut suffix = 1;

        while self.get_character_path(&candidate).exists() {
            candidate = format!("{}{}", base, suffix);
            suffix += 1;
        }

        candidate
    }

    fn default_chat_file_stem(name: &str) -> String {
        format!("{} - {}", name, humanized_chat_date(Utc::now()))
    }

    fn normalize_chat_file_stem(chat_name: &str, character_name: &str) -> String {
        let normalized = sanitize_filename(chat_name.trim().trim_end_matches(".jsonl"));
        if normalized.is_empty() {
            sanitize_filename(&Self::default_chat_file_stem(character_name))
        } else {
            normalized
        }
    }

    async fn persist_character_card(
        &self,
        file_stem: &str,
        base_image_data: &[u8],
        character: &Character,
    ) -> Result<PathBuf, DomainError> {
        let card_json = serde_json::to_string(&character.to_v2()).map_err(|e| {
            DomainError::InvalidData(format!("Failed to serialize character card: {}", e))
        })?;

        let image_data = write_character_data_to_png(base_image_data, &card_json)?;
        let target_path = self.get_character_path(file_stem);

        fs::write(&target_path, image_data).await.map_err(|e| {
            DomainError::InternalError(format!(
                "Failed to write imported character file {}: {}",
                target_path.display(),
                e
            ))
        })?;

        Ok(target_path)
    }

    pub(crate) async fn import_from_png_file(
        &self,
        source_path: &Path,
        file_data: &[u8],
        preserve_file_name: Option<&str>,
    ) -> Result<Character, DomainError> {
        let card_json = read_character_data_from_png(file_data)?;
        let mut character = self.parse_imported_character_json(&card_json)?;
        let file_stem =
            self.resolve_import_file_stem(&character, source_path, preserve_file_name)?;

        character.file_name = Some(file_stem.clone());
        character.avatar = format!("{}.png", file_stem);
        character.chat = Self::normalize_chat_file_stem(&character.chat, &character.name);

        let target_path = self
            .persist_character_card(&file_stem, file_data, &character)
            .await?;

        self.read_character_from_file(&target_path).await
    }

    pub(crate) async fn import_from_json_file(
        &self,
        source_path: &Path,
        file_data: Vec<u8>,
        preserve_file_name: Option<&str>,
    ) -> Result<Character, DomainError> {
        let card_json = String::from_utf8(file_data).map_err(|e| {
            DomainError::InvalidData(format!("Failed to decode JSON character file: {}", e))
        })?;
        let mut character = self.parse_imported_character_json(&card_json)?;
        let file_stem =
            self.resolve_import_file_stem(&character, source_path, preserve_file_name)?;

        character.file_name = Some(file_stem.clone());
        character.avatar = format!("{}.png", file_stem);
        character.chat = Self::normalize_chat_file_stem(&character.chat, &character.name);

        let default_avatar = self.read_default_avatar().await?;
        let target_path = self
            .persist_character_card(&file_stem, &default_avatar, &character)
            .await?;

        self.read_character_from_file(&target_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::FileCharacterRepository;

    #[test]
    fn normalize_json_surrogate_escapes_replaces_lone_high_surrogate() {
        let input = r#"{"first_mes":"Hello \uD83D"}"#;
        let normalized = FileCharacterRepository::normalize_json_surrogate_escapes(input);

        assert_eq!(normalized.as_ref(), r#"{"first_mes":"Hello \uFFFD"}"#);
    }

    #[test]
    fn normalize_json_surrogate_escapes_replaces_lone_low_surrogate() {
        let input = r#"{"first_mes":"Hello \uDE00"}"#;
        let normalized = FileCharacterRepository::normalize_json_surrogate_escapes(input);

        assert_eq!(normalized.as_ref(), r#"{"first_mes":"Hello \uFFFD"}"#);
    }

    #[test]
    fn normalize_json_surrogate_escapes_keeps_valid_surrogate_pair() {
        let input = r#"{"first_mes":"Hello \uD83D\uDE00"}"#;
        let normalized = FileCharacterRepository::normalize_json_surrogate_escapes(input);

        assert_eq!(normalized.as_ref(), input);
    }

    #[test]
    fn normalize_json_surrogate_escapes_skips_escaped_unicode_literal() {
        let input = r#"{"first_mes":"Literal \\uD83D marker"}"#;
        let normalized = FileCharacterRepository::normalize_json_surrogate_escapes(input);

        assert_eq!(normalized.as_ref(), input);
    }
}
