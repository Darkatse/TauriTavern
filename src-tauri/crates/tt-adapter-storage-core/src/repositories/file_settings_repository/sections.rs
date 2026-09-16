use std::fs;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::fields::{
    APPEARANCE_FILE, DYNAMIC_THEME_FILE, LAYOUT_FILE, PERSONA_STATE_FILE, PRESETS_FILE,
    UserSettingsSections,
};
use crate::file_system::persist_json_file_blocking;
use tt_domain::errors::DomainError;
use tt_domain::models::settings::{DynamicThemeSettings, TauriTavernSettings, UserSettings};

pub(super) fn load_user(root: &Path, defaults: &UserSettings) -> Result<UserSettings, DomainError> {
    let defaults = UserSettingsSections::split(defaults.data.clone());
    let core_path = root.join("settings.json");
    let stored = load_or_default(
        &core_path,
        &UserSettings {
            data: defaults.core,
        },
    )?;
    let mut sections = UserSettingsSections::split(stored.data);

    // Upgrade legacy settings, including imports over an existing installation.
    // Publish sections before removing their original fields from the core.
    let mut migrated = false;
    for (name, section, data) in [
        (
            APPEARANCE_FILE,
            &mut sections.appearance,
            defaults.appearance,
        ),
        (PRESETS_FILE, &mut sections.presets, defaults.presets),
        (LAYOUT_FILE, &mut sections.layout, defaults.layout),
        (
            PERSONA_STATE_FILE,
            &mut sections.persona_state,
            defaults.persona_state,
        ),
    ] {
        if section.as_object().is_some_and(|fields| !fields.is_empty()) {
            persist_changed(&root.join(name), section)?;
            migrated = true;
        } else {
            *section = load_or_default(&root.join(name), &UserSettings { data })?.data;
        }
    }
    if migrated {
        persist_json_file_blocking(&core_path, &sections.core)?;
    }
    Ok(UserSettings {
        data: sections.into_settings(),
    })
}

pub(super) fn save_user(root: &Path, settings: &UserSettings) -> Result<(), DomainError> {
    let sections = UserSettingsSections::split(settings.data.clone());
    persist_changed(&root.join(APPEARANCE_FILE), &sections.appearance)?;
    persist_changed(&root.join(PRESETS_FILE), &sections.presets)?;
    persist_changed(&root.join(LAYOUT_FILE), &sections.layout)?;
    persist_changed(&root.join(PERSONA_STATE_FILE), &sections.persona_state)?;
    persist_changed(&root.join("settings.json"), &sections.core)
}

pub(super) fn load_native(root: &Path) -> Result<TauriTavernSettings, DomainError> {
    let path = root.join("tauritavern-settings.json");
    let mut defaults = serde_json::to_value(TauriTavernSettings::default()).map_err(json_error)?;
    defaults
        .as_object_mut()
        .expect("settings object")
        .remove("dynamic_theme");
    let stored = read_optional::<Value>(&path)?;
    let missing_core = stored.is_none();
    let mut value = stored.unwrap_or(defaults);
    let legacy_theme = value
        .as_object_mut()
        .and_then(|map| map.remove("dynamic_theme"));
    let mut settings =
        TauriTavernSettings::from_json_value_with_compat(value.clone()).map_err(json_error)?;
    let theme_path = root.join(DYNAMIC_THEME_FILE);
    if let Some(legacy) = &legacy_theme {
        persist_changed(&theme_path, legacy)?;
    }
    settings.dynamic_theme = load_or_default(&theme_path, &DynamicThemeSettings::default())?;
    if missing_core || legacy_theme.is_some() {
        persist_json_file_blocking(&path, &value)?;
    }
    Ok(settings)
}

pub(super) fn save_native(root: &Path, settings: &TauriTavernSettings) -> Result<(), DomainError> {
    let mut core = serde_json::to_value(settings).map_err(json_error)?;
    let theme = core
        .as_object_mut()
        .expect("settings object")
        .remove("dynamic_theme");
    persist_changed(
        &root.join(DYNAMIC_THEME_FILE),
        &theme.expect("dynamic theme field"),
    )?;
    persist_changed(&root.join("tauritavern-settings.json"), &core)
}

fn persist_changed(path: &Path, value: &Value) -> Result<(), DomainError> {
    match read_optional::<Value>(path) {
        Ok(Some(stored)) if &stored == value => Ok(()),
        Ok(_) | Err(DomainError::InvalidData(_)) => persist_json_file_blocking(path, value),
        Err(error) => Err(error),
    }
}

fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, DomainError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read settings {}: {error}",
                path.display(),
            )));
        }
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        DomainError::InvalidData(format!("Invalid settings {}: {error}", path.display()))
    })
}

fn load_or_default<T: DeserializeOwned + Serialize + Clone>(
    path: &Path,
    defaults: &T,
) -> Result<T, DomainError> {
    match read_optional(path) {
        Ok(Some(value)) => return Ok(value),
        Ok(None) => {}
        Err(DomainError::InvalidData(error)) => {
            let name = path
                .file_name()
                .expect("settings filename")
                .to_string_lossy();
            let preserved = path.with_file_name(format!("{name}.corrupt-{}", uuid::Uuid::new_v4()));
            fs::rename(path, &preserved).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to preserve corrupt settings {}: {error}",
                    path.display(),
                ))
            })?;
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "{error}; original saved at {}; restoring this settings section to defaults",
                preserved.display(),
            );
        }
        Err(error) => return Err(error),
    }
    persist_json_file_blocking(path, defaults)?;
    Ok(defaults.clone())
}

fn json_error(error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid settings: {error}"))
}
