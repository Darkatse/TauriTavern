use crate::domain::errors::DomainError;
use serde_json::{Map, Value, json};

const DEFAULT_DEPTH: i64 = 4;
const DEFAULT_ROLE: i64 = 0;
const DEFAULT_SELECTIVE_LOGIC: i64 = 0;

pub(super) fn character_book_to_world_info(character_book: &Value) -> Result<Value, DomainError> {
    if let Some(entries_object) = character_book.get("entries").and_then(Value::as_object) {
        return Ok(json!({
            "entries": entries_object,
            "originalData": character_book,
        }));
    }

    let entries = character_book
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DomainError::InvalidData("Embedded character book has invalid entries".to_string())
        })?;

    let mut converted_entries = Map::new();
    for (index, entry) in entries.iter().enumerate() {
        let converted_entry = convert_character_book_entry(entry, index);
        let uid = converted_entry
            .get("uid")
            .and_then(Value::as_i64)
            .unwrap_or(index as i64);
        converted_entries.insert(uid.to_string(), converted_entry);
    }

    Ok(json!({
        "entries": converted_entries,
        "originalData": character_book,
    }))
}

pub(super) fn world_info_to_character_book(
    world_name: &str,
    world_info: &Value,
) -> Result<Value, DomainError> {
    let entries = world_info
        .get("entries")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            DomainError::InvalidData("World info file has invalid entries".to_string())
        })?;

    let converted_entries = entries
        .values()
        .enumerate()
        .map(|(index, entry)| convert_world_info_entry(entry, index))
        .collect::<Vec<_>>();

    if let Some(original_data) = world_info.get("originalData") {
        let mut character_book = original_data.as_object().cloned().ok_or_else(|| {
            DomainError::InvalidData(
                "World info originalData must be a character book object".to_string(),
            )
        })?;
        character_book.insert("name".to_string(), json!(world_name));
        character_book.insert("entries".to_string(), Value::Array(converted_entries));
        ensure_character_book_extensions(&mut character_book)?;
        return Ok(Value::Object(character_book));
    }

    Ok(json!({
        "name": world_name,
        "extensions": {},
        "entries": converted_entries,
    }))
}

fn ensure_character_book_extensions(
    character_book: &mut Map<String, Value>,
) -> Result<(), DomainError> {
    match character_book.get("extensions") {
        Some(Value::Object(_)) => Ok(()),
        Some(_) => Err(DomainError::InvalidData(
            "World info originalData character book extensions must be an object".to_string(),
        )),
        None => {
            character_book.insert("extensions".to_string(), Value::Object(Map::new()));
            Ok(())
        }
    }
}

fn convert_character_book_entry(entry: &Value, index: usize) -> Value {
    let id = entry
        .get("id")
        .and_then(Value::as_i64)
        .unwrap_or(index as i64);
    let comment = entry
        .get("comment")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let position = entry
        .pointer("/extensions/position")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            if entry.get("position").and_then(Value::as_str) == Some("before_char") {
                0
            } else {
                1
            }
        });
    let enabled = entry
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let extensions = entry
        .get("extensions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut result = Map::new();
    result.insert("uid".to_string(), json!(id));
    result.insert(
        "key".to_string(),
        json!(parse_string_array(entry.get("keys"))),
    );
    result.insert(
        "keysecondary".to_string(),
        json!(parse_string_array(entry.get("secondary_keys"))),
    );
    result.insert("comment".to_string(), json!(comment.clone()));
    result.insert(
        "content".to_string(),
        json!(entry.get("content").and_then(Value::as_str).unwrap_or("")),
    );
    result.insert(
        "constant".to_string(),
        json!(
            entry
                .get("constant")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "selective".to_string(),
        json!(
            entry
                .get("selective")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "order".to_string(),
        json!(
            entry
                .get("insertion_order")
                .and_then(Value::as_i64)
                .unwrap_or(100)
        ),
    );
    result.insert("position".to_string(), json!(position));
    result.insert("disable".to_string(), json!(!enabled));
    result.insert("addMemo".to_string(), json!(!comment.is_empty()));
    result.insert(
        "excludeRecursion".to_string(),
        json!(
            entry
                .pointer("/extensions/exclude_recursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "preventRecursion".to_string(),
        json!(
            entry
                .pointer("/extensions/prevent_recursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "delayUntilRecursion".to_string(),
        json!(
            entry
                .pointer("/extensions/delay_until_recursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "displayIndex".to_string(),
        json!(
            entry
                .pointer("/extensions/display_index")
                .and_then(Value::as_i64)
                .unwrap_or(index as i64)
        ),
    );
    result.insert(
        "probability".to_string(),
        entry
            .pointer("/extensions/probability")
            .cloned()
            .unwrap_or(json!(100)),
    );
    result.insert(
        "useProbability".to_string(),
        json!(
            entry
                .pointer("/extensions/useProbability")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        ),
    );
    result.insert(
        "depth".to_string(),
        json!(
            entry
                .pointer("/extensions/depth")
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_DEPTH)
        ),
    );
    result.insert(
        "selectiveLogic".to_string(),
        json!(
            entry
                .pointer("/extensions/selectiveLogic")
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_SELECTIVE_LOGIC)
        ),
    );
    result.insert(
        "outletName".to_string(),
        json!(
            entry
                .pointer("/extensions/outlet_name")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
    );
    result.insert(
        "group".to_string(),
        json!(
            entry
                .pointer("/extensions/group")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
    );
    result.insert(
        "groupOverride".to_string(),
        json!(
            entry
                .pointer("/extensions/group_override")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "groupWeight".to_string(),
        entry
            .pointer("/extensions/group_weight")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "scanDepth".to_string(),
        entry
            .pointer("/extensions/scan_depth")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "caseSensitive".to_string(),
        entry
            .pointer("/extensions/case_sensitive")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "matchWholeWords".to_string(),
        entry
            .pointer("/extensions/match_whole_words")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "useGroupScoring".to_string(),
        entry
            .pointer("/extensions/use_group_scoring")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "automationId".to_string(),
        json!(
            entry
                .pointer("/extensions/automation_id")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
    );
    result.insert(
        "role".to_string(),
        json!(
            entry
                .pointer("/extensions/role")
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_ROLE)
        ),
    );
    result.insert(
        "vectorized".to_string(),
        json!(
            entry
                .pointer("/extensions/vectorized")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "sticky".to_string(),
        entry
            .pointer("/extensions/sticky")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "cooldown".to_string(),
        entry
            .pointer("/extensions/cooldown")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "delay".to_string(),
        entry
            .pointer("/extensions/delay")
            .cloned()
            .unwrap_or(Value::Null),
    );
    result.insert(
        "matchPersonaDescription".to_string(),
        json!(
            entry
                .pointer("/extensions/match_persona_description")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "matchCharacterDescription".to_string(),
        json!(
            entry
                .pointer("/extensions/match_character_description")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "matchCharacterPersonality".to_string(),
        json!(
            entry
                .pointer("/extensions/match_character_personality")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "matchCharacterDepthPrompt".to_string(),
        json!(
            entry
                .pointer("/extensions/match_character_depth_prompt")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "matchScenario".to_string(),
        json!(
            entry
                .pointer("/extensions/match_scenario")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert(
        "matchCreatorNotes".to_string(),
        json!(
            entry
                .pointer("/extensions/match_creator_notes")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    result.insert("extensions".to_string(), Value::Object(extensions));
    result.insert(
        "triggers".to_string(),
        json!(parse_string_array(entry.pointer("/extensions/triggers"))),
    );
    result.insert(
        "ignoreBudget".to_string(),
        json!(
            entry
                .pointer("/extensions/ignore_budget")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );

    Value::Object(result)
}

fn convert_world_info_entry(entry: &Value, index: usize) -> Value {
    let id = entry
        .get("uid")
        .and_then(Value::as_i64)
        .unwrap_or(index as i64);
    let position = entry.get("position").and_then(Value::as_i64).unwrap_or(1);
    let mut extensions = entry
        .get("extensions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    extensions.insert("position".to_string(), json!(position));
    extensions.insert(
        "exclude_recursion".to_string(),
        json!(
            entry
                .get("excludeRecursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "display_index".to_string(),
        json!(
            entry
                .get("displayIndex")
                .and_then(Value::as_i64)
                .unwrap_or(index as i64)
        ),
    );
    extensions.insert(
        "probability".to_string(),
        entry.get("probability").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "useProbability".to_string(),
        json!(
            entry
                .get("useProbability")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "depth".to_string(),
        json!(
            entry
                .get("depth")
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_DEPTH)
        ),
    );
    extensions.insert(
        "selectiveLogic".to_string(),
        json!(
            entry
                .get("selectiveLogic")
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_SELECTIVE_LOGIC)
        ),
    );
    extensions.insert(
        "outlet_name".to_string(),
        json!(
            entry
                .get("outletName")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
    );
    extensions.insert(
        "group".to_string(),
        json!(entry.get("group").and_then(Value::as_str).unwrap_or("")),
    );
    extensions.insert(
        "group_override".to_string(),
        json!(
            entry
                .get("groupOverride")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "group_weight".to_string(),
        entry.get("groupWeight").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "prevent_recursion".to_string(),
        json!(
            entry
                .get("preventRecursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "delay_until_recursion".to_string(),
        json!(
            entry
                .get("delayUntilRecursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "scan_depth".to_string(),
        entry.get("scanDepth").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "match_whole_words".to_string(),
        entry.get("matchWholeWords").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "use_group_scoring".to_string(),
        json!(
            entry
                .get("useGroupScoring")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "case_sensitive".to_string(),
        entry.get("caseSensitive").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "automation_id".to_string(),
        json!(
            entry
                .get("automationId")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
    );
    extensions.insert(
        "role".to_string(),
        json!(
            entry
                .get("role")
                .and_then(Value::as_i64)
                .unwrap_or(DEFAULT_ROLE)
        ),
    );
    extensions.insert(
        "vectorized".to_string(),
        json!(
            entry
                .get("vectorized")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "sticky".to_string(),
        entry.get("sticky").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "cooldown".to_string(),
        entry.get("cooldown").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "delay".to_string(),
        entry.get("delay").cloned().unwrap_or(Value::Null),
    );
    extensions.insert(
        "match_persona_description".to_string(),
        json!(
            entry
                .get("matchPersonaDescription")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "match_character_description".to_string(),
        json!(
            entry
                .get("matchCharacterDescription")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "match_character_personality".to_string(),
        json!(
            entry
                .get("matchCharacterPersonality")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "match_character_depth_prompt".to_string(),
        json!(
            entry
                .get("matchCharacterDepthPrompt")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "match_scenario".to_string(),
        json!(
            entry
                .get("matchScenario")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "match_creator_notes".to_string(),
        json!(
            entry
                .get("matchCreatorNotes")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    extensions.insert(
        "triggers".to_string(),
        json!(parse_string_array(entry.get("triggers"))),
    );
    extensions.insert(
        "ignore_budget".to_string(),
        json!(
            entry
                .get("ignoreBudget")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );

    json!({
        "id": id,
        "keys": parse_string_array(entry.get("key")),
        "secondary_keys": parse_string_array(entry.get("keysecondary")),
        "comment": entry.get("comment").and_then(Value::as_str).unwrap_or(""),
        "content": entry.get("content").and_then(Value::as_str).unwrap_or(""),
        "constant": entry.get("constant").and_then(Value::as_bool).unwrap_or(false),
        "selective": entry.get("selective").and_then(Value::as_bool).unwrap_or(false),
        "insertion_order": entry.get("order").and_then(Value::as_i64).unwrap_or(100),
        "enabled": !entry.get("disable").and_then(Value::as_bool).unwrap_or(false),
        "position": if position == 0 { "before_char" } else { "after_char" },
        "use_regex": true,
        "extensions": extensions,
    })
}

fn parse_string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{character_book_to_world_info, parse_string_array, world_info_to_character_book};
    use serde_json::{Value, json};

    #[test]
    fn character_book_to_world_info_builds_world_info_structure() {
        let character_book: Value = serde_json::from_str(
            r#"{
                "name": "Lore",
                "entries": [{
                    "id": 42,
                    "keys": ["alpha", "beta"],
                    "secondary_keys": ["gamma"],
                    "comment": "memo",
                    "content": "content",
                    "constant": true,
                    "insertion_order": 150,
                    "enabled": true,
                    "position": "before_char",
                    "extensions": {
                        "position": 0,
                        "exclude_recursion": true,
                        "prevent_recursion": true,
                        "display_index": 7,
                        "probability": 88,
                        "depth": 6,
                        "role": 2,
                        "triggers": ["hello"],
                        "custom": "value"
                    }
                }]
            }"#,
        )
        .expect("character book json should parse");

        let converted =
            character_book_to_world_info(&character_book).expect("conversion should succeed");
        let entry = converted
            .pointer("/entries/42")
            .expect("converted entry should exist");

        assert_eq!(entry.get("uid").and_then(|value| value.as_i64()), Some(42));
        assert_eq!(entry.get("key"), Some(&json!(["alpha", "beta"])));
        assert_eq!(entry.get("keysecondary"), Some(&json!(["gamma"])));
        assert_eq!(
            entry.get("position").and_then(|value| value.as_i64()),
            Some(0)
        );
        assert_eq!(
            entry
                .get("excludeRecursion")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            entry
                .get("preventRecursion")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            entry
                .pointer("/extensions/custom")
                .and_then(|value| value.as_str()),
            Some("value")
        );
        assert_eq!(
            converted.get("originalData"),
            Some(&character_book),
            "original character_book should be preserved"
        );
    }

    #[test]
    fn world_info_to_character_book_converts_entries_without_original_data() {
        let world_info = json!({
            "entries": {
                "42": {
                    "uid": 42,
                    "key": ["alpha", "beta"],
                    "keysecondary": ["gamma"],
                    "comment": "memo",
                    "content": "content",
                    "constant": true,
                    "order": 150,
                    "position": 0,
                    "disable": false,
                    "extensions": {
                        "custom": "value"
                    },
                    "excludeRecursion": true,
                    "preventRecursion": true,
                    "displayIndex": 7,
                    "probability": 88,
                    "depth": 6,
                    "role": 2,
                    "triggers": ["hello"]
                }
            }
        });

        let converted =
            world_info_to_character_book("Lore", &world_info).expect("conversion should succeed");
        let entry = converted
            .pointer("/entries/0")
            .expect("converted character book entry should exist");

        assert_eq!(converted.get("name"), Some(&json!("Lore")));
        assert_eq!(entry.get("id"), Some(&json!(42)));
        assert_eq!(entry.get("keys"), Some(&json!(["alpha", "beta"])));
        assert_eq!(entry.get("secondary_keys"), Some(&json!(["gamma"])));
        assert_eq!(entry.get("position"), Some(&json!("before_char")));
        assert_eq!(entry.pointer("/extensions/custom"), Some(&json!("value")));
        assert_eq!(
            entry.pointer("/extensions/triggers"),
            Some(&json!(["hello"]))
        );
    }

    #[test]
    fn world_info_to_character_book_merges_original_data_with_current_entries() {
        let original_data = json!({
            "name": "Imported Lore",
            "description": "preserve me",
            "entries": [{ "id": 1, "keys": ["alpha"], "content": "stale" }]
        });
        let world_info = json!({
            "entries": {
                "7": {
                    "uid": 7,
                    "key": ["beta"],
                    "comment": "memo",
                    "content": "fresh",
                    "order": 33,
                    "position": 1,
                    "disable": false,
                    "extensions": {
                        "custom": "value"
                    }
                }
            },
            "originalData": original_data,
        });

        let converted =
            world_info_to_character_book("Lore", &world_info).expect("conversion should succeed");

        assert_eq!(converted.get("name"), Some(&json!("Lore")));
        assert_eq!(converted.get("description"), Some(&json!("preserve me")));
        assert_eq!(converted.pointer("/entries/0/id"), Some(&json!(7)));
        assert_eq!(
            converted.pointer("/entries/0/content"),
            Some(&json!("fresh"))
        );
        assert_eq!(
            converted.pointer("/entries/0/extensions/custom"),
            Some(&json!("value"))
        );
    }

    #[test]
    fn parse_string_array_accepts_array_and_csv() {
        let from_array = parse_string_array(Some(&json!(["a", " b ", ""])));
        let from_csv = parse_string_array(Some(&json!("x, y , ,z")));

        assert_eq!(from_array, vec!["a", "b"]);
        assert_eq!(from_csv, vec!["x", "y", "z"]);
    }
}
