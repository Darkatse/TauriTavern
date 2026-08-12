use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};
use tt_domain::errors::DomainError;

use super::super::shared::{IMPORT_TARGET_USER_HANDLE, internal_error};

const SETTINGS_FILE: &str = "settings.json";
const USER_AVATARS_DIR: &str = "User Avatars";

/// Completes the staged source registry for avatar files the incremental overlay will retain.
pub(super) fn merge_retained_personas(
    normalized_root: &Path,
    data_root: &Path,
) -> Result<(), DomainError> {
    let source_settings_path = normalized_root
        .join(IMPORT_TARGET_USER_HANDLE)
        .join(SETTINGS_FILE);
    let target_user_path = data_root.join(IMPORT_TARGET_USER_HANDLE);
    let target_settings_path = target_user_path.join(SETTINGS_FILE);
    if !source_settings_path.is_file() || !target_settings_path.is_file() {
        return Ok(());
    }

    let mut source_settings = read_settings(&source_settings_path, "imported")?;
    let target_settings = match read_settings(&target_settings_path, "existing") {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(
                "Skipping existing Persona registry during data import: {}",
                error
            );
            return Ok(());
        }
    };
    let avatar_ids = match read_avatar_ids(&target_user_path.join(USER_AVATARS_DIR)) {
        Ok(avatar_ids) => avatar_ids,
        Err(error) => {
            tracing::warn!(
                "Skipping existing Persona registry during data import: {}",
                error
            );
            return Ok(());
        }
    };

    if merge_missing_personas(&mut source_settings, &target_settings, &avatar_ids)? {
        let bytes = serde_json::to_vec_pretty(&source_settings)
            .map_err(|error| internal_error("Failed to serialize imported settings", error))?;
        fs::write(&source_settings_path, bytes)
            .map_err(|error| internal_error("Failed to stage merged imported settings", error))?;
    }

    Ok(())
}

fn read_settings(path: &Path, label: &str) -> Result<Value, DomainError> {
    let bytes = fs::read(path)
        .map_err(|error| internal_error(&format!("Failed to read {label} settings"), error))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid {label} settings file {}: {error}",
            path.display()
        ))
    })
}

fn read_avatar_ids(directory: &Path) -> Result<HashSet<String>, DomainError> {
    if !directory.is_dir() {
        return Ok(HashSet::new());
    }

    let mut avatar_ids = HashSet::new();
    for entry in fs::read_dir(directory)
        .map_err(|error| internal_error("Failed to read existing Persona avatars", error))?
    {
        let entry = entry
            .map_err(|error| internal_error("Failed to read existing Persona avatar", error))?;
        if entry
            .file_type()
            .map_err(|error| internal_error("Failed to inspect existing Persona avatar", error))?
            .is_file()
            && let Some(avatar_id) = entry.file_name().to_str()
        {
            avatar_ids.insert(avatar_id.to_string());
        }
    }

    Ok(avatar_ids)
}

fn merge_missing_personas(
    source_settings: &mut Value,
    target_settings: &Value,
    avatar_ids: &HashSet<String>,
) -> Result<bool, DomainError> {
    let Some(target_personas) = settings_map(target_settings, "personas") else {
        return Ok(false);
    };
    let target_descriptions = settings_map(target_settings, "persona_descriptions");
    let source_power_user = power_user_mut(source_settings)?;
    let source_personas = map_mut(source_power_user, "personas")?;
    let missing = target_personas
        .iter()
        .filter(|(avatar_id, name)| {
            name.is_string()
                && avatar_ids.contains(*avatar_id)
                && !source_personas.contains_key(*avatar_id)
        })
        .map(|(avatar_id, name)| (avatar_id.clone(), name.clone()))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(false);
    }

    for (avatar_id, name) in &missing {
        source_personas.insert(avatar_id.clone(), name.clone());
    }

    if let Some(target_descriptions) = target_descriptions {
        let source_descriptions = map_mut(source_power_user, "persona_descriptions")?;
        for (avatar_id, _) in missing {
            if let Some(description) = target_descriptions.get(&avatar_id) {
                source_descriptions.insert(avatar_id, description.clone());
            }
        }
    }

    Ok(true)
}

fn settings_map<'a>(settings: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    settings
        .get("power_user")?
        .as_object()?
        .get(key)?
        .as_object()
}

fn power_user_mut(settings: &mut Value) -> Result<&mut Map<String, Value>, DomainError> {
    let settings = settings.as_object_mut().ok_or_else(|| {
        DomainError::InvalidData("Imported settings must be a JSON object".to_string())
    })?;
    map_mut(settings, "power_user")
}

fn map_mut<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, DomainError> {
    parent
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            DomainError::InvalidData(format!("Imported settings field must be an object: {key}"))
        })
}
