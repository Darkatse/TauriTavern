use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::errors::DomainError;
use crate::json_document::JsonDocument;

const POWER_USER: &str = "power_user";
const NAMES: &str = "personas";
const DESCRIPTIONS: &str = "persona_descriptions";

/// The original name and descriptor entries, carried together by one avatar file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Persona {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
}

pub type Personas = BTreeMap<String, Persona>;

/// `None` when the settings hold neither Persona table.
pub fn take_personas(settings: &mut JsonDocument) -> Result<Option<Personas>, DomainError> {
    let Some(power_user) = settings
        .object_mut(&[POWER_USER])
        .map_err(|error| invalid_settings(POWER_USER, error))?
    else {
        return Ok(None);
    };
    let names: Option<BTreeMap<String, String>> = take_table(power_user, NAMES)?;
    let descriptions: Option<BTreeMap<String, Value>> = take_table(power_user, DESCRIPTIONS)?;
    if names.is_none() && descriptions.is_none() {
        return Ok(None);
    }
    let mut personas = Personas::new();
    for (id, name) in names.into_iter().flatten() {
        personas.entry(id).or_default().name = Some(name);
    }
    for (id, description) in descriptions.into_iter().flatten() {
        personas.entry(id).or_default().description = Some(description);
    }
    Ok(Some(personas))
}

fn take_table<T: DeserializeOwned>(
    power_user: &mut JsonDocument,
    field: &str,
) -> Result<Option<T>, DomainError> {
    let table = power_user
        .get(&[field])
        .map_err(|error| invalid_settings(&format!("{POWER_USER}.{field}"), error))?;
    power_user.remove(field);
    Ok(table)
}

pub fn insert_personas(
    settings: &mut JsonDocument,
    personas: &Personas,
) -> Result<(), DomainError> {
    let mut names = Map::new();
    let mut descriptions = Map::new();
    for (id, persona) in personas {
        if let Some(name) = &persona.name {
            names.insert(id.clone(), Value::String(name.clone()));
        }
        if let Some(description) = &persona.description {
            descriptions.insert(id.clone(), description.clone());
        }
    }
    let power_user = settings
        .object_entry(&[POWER_USER])
        .map_err(|error| invalid_settings(POWER_USER, error))?;
    power_user.set(NAMES, &Value::Object(names));
    power_user.set(DESCRIPTIONS, &Value::Object(descriptions));
    Ok(())
}

fn invalid_settings(field: &str, error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid settings {field}: {error}"))
}
