use crate::errors::ApplicationError;
use serde_json::Value;
use tt_domain::errors::DomainError;

const EMBEDDED_AGENT_PROFILES_VERSION: u64 = 1;

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

pub(super) fn validate_character_card_schema(card_value: &Value) -> Result<(), DomainError> {
    if validate_v1_character_card(card_value).is_ok()
        || validate_v2_character_card(card_value).is_ok()
    {
        return Ok(());
    }

    validate_v3_character_card(card_value)
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

pub(super) fn set_character_world(
    card_value: &mut Value,
    world: impl Into<String>,
) -> Result<(), DomainError> {
    let root = card_value.as_object_mut().ok_or_else(|| {
        DomainError::InvalidData("Character payload must be a JSON object".to_string())
    })?;
    let data = root
        .entry("data")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid_character_card_field("data"))?;
    let extensions = data
        .entry("extensions")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid_character_card_field("data.extensions"))?;
    extensions.insert("world".to_string(), Value::String(world.into()));
    Ok(())
}

pub(super) fn unset_private_fields(export_value: &mut Value) -> Result<(), DomainError> {
    let Some(root_object) = export_value.as_object_mut() else {
        return Err(DomainError::InvalidData(
            "Character payload must be a JSON object".to_string(),
        ));
    };

    root_object.insert("fav".to_string(), Value::Bool(false));
    root_object.remove("chat");

    if let Some(extensions) = root_object
        .get_mut("data")
        .and_then(Value::as_object_mut)
        .and_then(|data| data.get_mut("extensions"))
        .and_then(Value::as_object_mut)
    {
        extensions.insert("fav".to_string(), Value::Bool(false));
    }

    Ok(())
}

pub(super) fn sanitize_agent_profiles_for_export(export_value: &mut Value) {
    sanitize_agent_profile_package_at_path(export_value, "/data/extensions");
    sanitize_agent_profile_package_at_path(export_value, "/extensions");
}

fn sanitize_agent_profile_package_at_path(value: &mut Value, extension_path: &str) {
    let Some(extensions) = value
        .pointer_mut(extension_path)
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(tauritavern) = extensions.get_mut("tauritavern") else {
        return;
    };
    let Some(tauritavern) = tauritavern.as_object_mut() else {
        return;
    };
    let Some(error) = tauritavern
        .get_mut("agentProfiles")
        .and_then(|package| sanitize_agent_profile_package(package).err())
    else {
        return;
    };
    tracing::error!(
        target: tt_contracts::observability::USER_VISIBLE_ERROR,
        "Exporting character without malformed embedded Agent Profiles: {}",
        error
    );
    tauritavern.remove("agentProfiles");
}

fn sanitize_agent_profile_package(package: &mut Value) -> Result<(), DomainError> {
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
    for item in items {
        sanitize_agent_profile_item(item)?;
    }
    Ok(())
}

fn sanitize_agent_profile_item(item: &mut Value) -> Result<(), DomainError> {
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
    if profile
        .get("model")
        .and_then(Value::as_object)
        .and_then(|model| model.get("mode"))
        .and_then(Value::as_str)
        == Some("connectionRef")
    {
        profile.insert(
            "model".to_string(),
            serde_json::json!({ "mode": "requiresConfiguration" }),
        );
    }
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn character_export_sanitizes_embedded_agent_profile_model_bindings() {
        let mut card = json!({
            "data": {
                "extensions": {
                    "tauritavern": {
                        "agentProfiles": {
                            "version": 1,
                            "items": [
                                {
                                    "profile": {
                                        "id": "writer",
                                        "model": {
                                            "mode": "connectionRef",
                                            "connectionRef": "model-target-private",
                                            "modelId": "private-model"
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });

        super::sanitize_agent_profiles_for_export(&mut card);

        assert_eq!(
            card["data"]["extensions"]["tauritavern"]["agentProfiles"]["items"][0]["profile"]["model"],
            json!({ "mode": "requiresConfiguration" })
        );
    }

    #[test]
    fn character_export_drops_malformed_embedded_agent_profile_package() {
        let mut unsupported_version = json!({
            "data": {
                "extensions": {
                    "tauritavern": {
                        "agentProfiles": {
                            "version": 2,
                            "items": []
                        }
                    }
                }
            }
        });
        super::sanitize_agent_profiles_for_export(&mut unsupported_version);
        assert!(
            unsupported_version
                .pointer("/data/extensions/tauritavern/agentProfiles")
                .is_none()
        );

        let mut missing_items = json!({
            "data": {
                "extensions": {
                    "tauritavern": {
                        "agentProfiles": {
                            "version": 1
                        }
                    }
                }
            }
        });
        super::sanitize_agent_profiles_for_export(&mut missing_items);
        assert!(
            missing_items
                .pointer("/data/extensions/tauritavern/agentProfiles")
                .is_none()
        );

        let mut missing_profile = json!({
            "data": {
                "extensions": {
                    "tauritavern": {
                        "agentProfiles": {
                            "version": 1,
                            "items": [
                                {}
                            ]
                        }
                    }
                }
            }
        });
        super::sanitize_agent_profiles_for_export(&mut missing_profile);
        assert!(
            missing_profile
                .pointer("/data/extensions/tauritavern/agentProfiles")
                .is_none()
        );
    }
}
