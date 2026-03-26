use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::character::Character;
use serde_json::Value;

pub(super) fn parse_character_card_json(card_json: &str) -> Result<Value, ApplicationError> {
    let value: Value = serde_json::from_str(card_json).map_err(|error| {
        ApplicationError::ValidationError(format!("Invalid character card JSON: {}", error))
    })?;

    if !value.is_object() {
        return Err(ApplicationError::ValidationError(
            "Character card payload must be a JSON object".to_string(),
        ));
    }

    Ok(value)
}

pub(super) fn strip_character_card_json_data(card_value: &mut Value) {
    if let Some(card_object) = card_value.as_object_mut() {
        card_object.remove("json_data");
    }
}

pub(super) fn ensure_readable_character_card(card_value: &Value) -> Result<(), ApplicationError> {
    serde_json::from_value::<Character>(card_value.clone()).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "Character card payload is not readable: {}",
            error
        ))
    })?;
    Ok(())
}

pub(super) fn normalize_v2_character_book_extensions(
    card_value: &mut Value,
) -> Result<(), DomainError> {
    if card_value.get("spec").and_then(Value::as_str) != Some("chara_card_v2") {
        return Ok(());
    }

    let Some(character_book) = card_value.pointer_mut("/data/character_book") else {
        return Ok(());
    };
    let Some(character_book_object) = character_book.as_object_mut() else {
        return Err(invalid_character_card_field("data.character_book"));
    };

    match character_book_object.get("extensions") {
        Some(Value::Object(_)) => Ok(()),
        Some(_) => Err(invalid_character_card_field(
            "data.character_book.extensions",
        )),
        None => {
            character_book_object.insert(
                "extensions".to_string(),
                Value::Object(serde_json::Map::new()),
            );
            Ok(())
        }
    }
}

pub(super) fn validate_character_card_schema(card_value: &Value) -> Result<(), DomainError> {
    match card_value.get("spec").and_then(Value::as_str) {
        Some("chara_card_v2") => validate_v2_character_card(card_value)?,
        Some("chara_card_v3") => validate_v3_character_card(card_value)?,
        Some(_) => return Err(invalid_character_card_field("spec")),
        None => validate_v1_character_card(card_value)?,
    }

    Ok(())
}

pub(super) fn character_card_name(card_value: &Value) -> Result<&str, DomainError> {
    card_value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            card_value
                .pointer("/data/name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| missing_character_card_field("name"))
}

pub(super) fn unset_private_fields(export_value: &mut Value) -> Result<(), DomainError> {
    let Some(root_object) = export_value.as_object_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload must be a JSON object".to_string(),
        ));
    };

    root_object.insert("fav".to_string(), Value::Bool(false));
    root_object.remove("chat");

    let data = root_object
        .entry("data")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(data_object) = data.as_object_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload data must be a JSON object".to_string(),
        ));
    };

    let extensions = data_object
        .entry("extensions")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(extensions_object) = extensions.as_object_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload extensions must be a JSON object".to_string(),
        ));
    };

    extensions_object.insert("fav".to_string(), Value::Bool(false));

    Ok(())
}

fn validate_v1_character_card(card_value: &Value) -> Result<(), DomainError> {
    for field in [
        "name",
        "description",
        "personality",
        "scenario",
        "first_mes",
        "mes_example",
    ] {
        if card_value.get(field).is_none() {
            return Err(missing_character_card_field(field));
        }
    }

    Ok(())
}

fn validate_v2_character_card(card_value: &Value) -> Result<(), DomainError> {
    if card_value.get("spec_version").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid_character_card_field("spec_version"));
    }

    let Some(data) = card_value.get("data").and_then(Value::as_object) else {
        return Err(missing_character_card_field("data"));
    };

    for field in [
        "name",
        "description",
        "personality",
        "scenario",
        "first_mes",
        "mes_example",
        "creator_notes",
        "system_prompt",
        "post_history_instructions",
        "alternate_greetings",
        "tags",
        "creator",
        "character_version",
        "extensions",
    ] {
        if !data.contains_key(field) {
            return Err(missing_character_card_field(&format!("data.{}", field)));
        }
    }

    if !data.get("alternate_greetings").is_some_and(Value::is_array) {
        return Err(invalid_character_card_field("data.alternate_greetings"));
    }

    if !data.get("tags").is_some_and(Value::is_array) {
        return Err(invalid_character_card_field("data.tags"));
    }

    if !data.get("extensions").is_some_and(Value::is_object) {
        return Err(invalid_character_card_field("data.extensions"));
    }

    if let Some(character_book) = data.get("character_book") {
        let Some(character_book) = character_book.as_object() else {
            return Err(invalid_character_card_field("data.character_book"));
        };

        if !character_book.contains_key("extensions") {
            return Err(missing_character_card_field(
                "data.character_book.extensions",
            ));
        }

        if !character_book.contains_key("entries") {
            return Err(missing_character_card_field("data.character_book.entries"));
        }

        if !character_book
            .get("extensions")
            .is_some_and(Value::is_object)
        {
            return Err(invalid_character_card_field(
                "data.character_book.extensions",
            ));
        }

        if !character_book.get("entries").is_some_and(Value::is_array) {
            return Err(invalid_character_card_field("data.character_book.entries"));
        }
    }

    Ok(())
}

fn validate_v3_character_card(card_value: &Value) -> Result<(), DomainError> {
    let spec_version = card_value
        .get("spec_version")
        .and_then(character_card_spec_version);

    if !spec_version.is_some_and(|value| (3.0..4.0).contains(&value)) {
        return Err(invalid_character_card_field("spec_version"));
    }

    if !card_value.get("data").is_some_and(Value::is_object) {
        return Err(missing_character_card_field("data"));
    }

    Ok(())
}

fn character_card_spec_version(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) => string.parse::<f64>().ok(),
        _ => None,
    }
}

fn missing_character_card_field(field: &str) -> DomainError {
    DomainError::InvalidData(format!("Character card field {} is required", field))
}

fn invalid_character_card_field(field: &str) -> DomainError {
    DomainError::InvalidData(format!("Character card field {} is invalid", field))
}
