use serde_json::Value;
use tt_domain::errors::DomainError;
use tt_domain::json_document::JsonDocument;

const EMBEDDED_AGENT_PROFILES_VERSION: u64 = 1;

pub(super) fn parse_character_card_json(card_json: &str) -> Result<JsonDocument, DomainError> {
    JsonDocument::parse(card_json.as_bytes()).map_err(invalid_character_card_json)
}

pub(super) fn invalid_character_card_json(error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid character card JSON: {error}"))
}

/// `json_data` carries the request's copy of the card and is never stored.
pub(super) fn into_character_card_json(mut card: JsonDocument) -> String {
    card.remove("json_data");
    card.to_string()
}

pub(super) fn validate_character_card_schema(card_json: &str) -> Result<(), DomainError> {
    let card_value: Value = serde_json::from_str(card_json).map_err(invalid_character_card_json)?;
    if validate_v1_character_card(&card_value).is_ok()
        || validate_v2_character_card(&card_value).is_ok()
    {
        return Ok(());
    }

    validate_v3_character_card(&card_value)
}

pub(super) fn character_world(card: &JsonDocument) -> Result<Option<String>, DomainError> {
    let world = card
        .get::<Value>(&["data", "extensions", "world"])
        .map_err(invalid_character_card_json)?;
    Ok(world
        .as_ref()
        .and_then(Value::as_str)
        .filter(|world| !world.is_empty())
        .map(str::to_owned))
}

pub(super) fn set_character_world(card: &mut JsonDocument, world: &str) -> Result<(), DomainError> {
    card.object_entry(&["data", "extensions"])
        .map_err(invalid_character_card_json)?
        .set("world", &Value::from(world));
    Ok(())
}

pub(super) fn unset_private_fields(card: &mut JsonDocument) -> Result<(), DomainError> {
    card.set("fav", &Value::Bool(false));
    card.remove("chat");
    if let Some(extensions) = card
        .object_mut(&["data", "extensions"])
        .map_err(invalid_character_card_json)?
    {
        extensions.set("fav", &Value::Bool(false));
    }
    Ok(())
}

pub(super) fn sanitize_agent_profiles_for_export(
    card: &mut JsonDocument,
) -> Result<(), DomainError> {
    for extensions in [&["data", "extensions"][..], &["extensions"]] {
        sanitize_agent_profile_package_in(card, extensions).map_err(invalid_character_card_json)?;
    }
    Ok(())
}

fn sanitize_agent_profile_package_in(
    card: &mut JsonDocument,
    extensions: &[&str],
) -> serde_json::Result<()> {
    let Some(tauritavern) = card.object_mut(&[extensions, &["tauritavern"]].concat())? else {
        return Ok(());
    };
    let Some(mut package) = tauritavern.get::<Value>(&["agentProfiles"])? else {
        return Ok(());
    };
    match sanitize_agent_profile_package(&mut package) {
        Ok(true) => tauritavern.set("agentProfiles", &package),
        Ok(false) => {}
        Err(error) => {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Exporting character without malformed embedded Agent Profiles: {}",
                error
            );
            tauritavern.remove("agentProfiles");
        }
    }
    Ok(())
}

/// Returns whether any profile changed.
fn sanitize_agent_profile_package(package: &mut Value) -> Result<bool, DomainError> {
    let Some(package) = package.as_object_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload tauritavern.agentProfiles must be an object".to_string(),
        ));
    };
    if package.get("version").and_then(Value::as_u64) != Some(EMBEDDED_AGENT_PROFILES_VERSION) {
        return Err(DomainError::InvalidData(
            "Character payload tauritavern.agentProfiles.version must be 1".to_string(),
        ));
    }
    let Some(items) = package.get_mut("items") else {
        return Err(DomainError::InvalidData(
            "Character payload tauritavern.agentProfiles.items must be an array".to_string(),
        ));
    };
    let Some(items) = items.as_array_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload tauritavern.agentProfiles.items must be an array".to_string(),
        ));
    };
    let mut changed = false;
    for item in items {
        changed |= sanitize_agent_profile_item(item)?;
    }
    Ok(changed)
}

fn sanitize_agent_profile_item(item: &mut Value) -> Result<bool, DomainError> {
    let Some(item) = item.as_object_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload embedded Agent Profile item must be an object".to_string(),
        ));
    };
    let Some(profile) = item.get_mut("profile") else {
        return Err(DomainError::InvalidData(
            "Character payload embedded Agent Profile item.profile must be an object".to_string(),
        ));
    };
    let Some(profile) = profile.as_object_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload embedded Agent Profile must be an object".to_string(),
        ));
    };
    let uses_connection = profile
        .get("model")
        .and_then(Value::as_object)
        .and_then(|model| model.get("mode"))
        .and_then(Value::as_str)
        == Some("connectionRef");
    if uses_connection {
        profile.insert(
            "model".to_string(),
            serde_json::json!({ "mode": "requiresConfiguration" }),
        );
    }
    Ok(uses_connection)
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
    if card_value.get("spec").and_then(Value::as_str) != Some("chara_card_v2") {
        return Err(invalid_character_card_field("spec"));
    }

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

    if !data.get("extensions").is_some_and(is_javascript_object) {
        return Err(invalid_character_card_field("data.extensions"));
    }

    if let Some(character_book) = data
        .get("character_book")
        .filter(|value| !is_javascript_falsy(value))
    {
        if character_book.get("extensions").is_none() {
            return Err(missing_character_card_field(
                "data.character_book.extensions",
            ));
        }

        if character_book.get("entries").is_none() {
            return Err(missing_character_card_field("data.character_book.entries"));
        }

        if !character_book
            .get("extensions")
            .is_some_and(is_javascript_object)
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
    if card_value.get("spec").and_then(Value::as_str) != Some("chara_card_v3") {
        return Err(invalid_character_card_field("spec"));
    }

    let spec_version = card_value
        .get("spec_version")
        .and_then(character_card_spec_version);

    if !spec_version.is_some_and(|value| (3.0..4.0).contains(&value)) {
        return Err(invalid_character_card_field("spec_version"));
    }

    if !card_value
        .get("data")
        .is_some_and(|value| !is_javascript_falsy(value) && is_javascript_object(value))
    {
        return Err(missing_character_card_field("data"));
    }

    Ok(())
}

fn character_card_spec_version(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) if string.trim().is_empty() => Some(0.0),
        Value::String(string) => string.trim().parse::<f64>().ok(),
        Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Value::Null => Some(0.0),
        _ => None,
    }
}

fn is_javascript_object(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Array(_) | Value::Object(_))
}

fn is_javascript_falsy(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Bool(false))
        || value.as_f64() == Some(0.0)
        || value.as_str() == Some("")
}

fn missing_character_card_field(field: &str) -> DomainError {
    DomainError::InvalidData(format!("Character card field {} is required", field))
}

fn invalid_character_card_field(field: &str) -> DomainError {
    DomainError::InvalidData(format!("Character card field {} is invalid", field))
}
