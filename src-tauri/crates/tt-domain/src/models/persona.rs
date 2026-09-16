use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::errors::DomainError;

/// The original name and descriptor entries, carried together by one avatar file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Persona {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
}

pub type Personas = BTreeMap<String, Persona>;

/// Remove the upstream registry projection without changing other settings.
pub fn take_personas(settings: &mut Value) -> Result<Option<Personas>, DomainError> {
    let Some(power_user) = settings
        .get_mut("power_user")
        .and_then(Value::as_object_mut)
    else {
        return Ok(None);
    };
    let names = power_user.remove("personas");
    let descriptions = power_user.remove("persona_descriptions");
    if names.is_none() && descriptions.is_none() {
        return Ok(None);
    }
    let mut personas = Personas::new();
    if let Some(names) = names {
        let names: BTreeMap<String, String> = serde_json::from_value(names)
            .map_err(|error| DomainError::InvalidData(format!("Invalid Persona names: {error}")))?;
        for (id, name) in names {
            personas.entry(id).or_default().name = Some(name);
        }
    }
    if let Some(descriptions) = descriptions {
        let descriptions: BTreeMap<String, Value> =
            serde_json::from_value(descriptions).map_err(|error| {
                DomainError::InvalidData(format!("Invalid Persona descriptions: {error}"))
            })?;
        for (id, description) in descriptions {
            personas.entry(id).or_default().description = Some(description);
        }
    }
    Ok(Some(personas))
}

pub fn insert_personas(settings: &mut Value, personas: &Personas) {
    if !settings["power_user"].is_object() {
        settings["power_user"] = json!({});
    }
    let mut names = serde_json::Map::new();
    let mut descriptions = serde_json::Map::new();
    for (id, persona) in personas {
        if let Some(name) = &persona.name {
            names.insert(id.clone(), Value::String(name.clone()));
        }
        if let Some(description) = &persona.description {
            descriptions.insert(id.clone(), description.clone());
        }
    }
    settings["power_user"]["personas"] = Value::Object(names);
    settings["power_user"]["persona_descriptions"] = Value::Object(descriptions);
}
