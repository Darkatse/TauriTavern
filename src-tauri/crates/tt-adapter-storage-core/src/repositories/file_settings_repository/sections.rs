use std::fs;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::fields::{
    DYNAMIC_THEME_FILE, NATIVE_FILE, PERSONA_STATE_FILE, SECTION_FILES, UserSettingsSections,
};
use crate::file_system::{
    KeptOriginal, persist_bytes_file_blocking, persist_changed_blocking,
    persist_json_file_blocking, replace_keeping_original_blocking,
};
use tt_domain::errors::DomainError;
use tt_domain::json_document::JsonDocument;
use tt_domain::models::persona::{Personas, take_personas_with};
use tt_domain::models::settings::repair::{
    PROMPT_MANAGER_LISTS, PROMPT_MANAGER_SETTINGS, UserSettingsRepairReport,
    repair_prompt_manager_object,
};
use tt_domain::models::settings::revision::UserSettingsRevision;
use tt_domain::models::settings::{DynamicThemeSettings, TauriTavernSettings, UserSettings};
use tt_ports::repositories::settings_repository::StoredUserSettings;

const REVISION_ALGORITHM: &str = "tt-user-settings-sections-sha256-v2";
const CORE: usize = SECTION_FILES.len() - 1;

pub(super) struct DefaultSections([Vec<u8>; 5]);

impl DefaultSections {
    pub(super) fn new(defaults: &UserSettings) -> Self {
        let sections = JsonDocument::from_serialize(&defaults.data)
            .and_then(UserSettingsSections::split)
            .expect("bundled default settings are a JSON object");
        Self(sections.into_files().map(|section| section.to_vec()))
    }
}

/// The section texts as stored, in [`SECTION_FILES`] order, with their digests.
pub(super) struct StoredSections {
    texts: [Vec<u8>; 5],
    digests: [[u8; 32]; 5],
}

impl StoredSections {
    /// Reads the files as they are, without normalizing; a missing file reads as empty.
    pub(super) fn read(root: &Path) -> Result<Self, DomainError> {
        let mut texts: [Vec<u8>; 5] = Default::default();
        for (text, name) in texts.iter_mut().zip(SECTION_FILES) {
            let path = root.join(name);
            match fs::read(&path) {
                Ok(bytes) => *text = bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(read_error(&path, error)),
            }
        }
        Ok(Self::new(texts))
    }

    fn new(texts: [Vec<u8>; 5]) -> Self {
        let digests = texts.each_ref().map(|text| Sha256::digest(text).into());
        Self { texts, digests }
    }

    /// The hash of the section hashes, so that a commit rehashes only the sections it changes.
    pub(super) fn revision(&self) -> UserSettingsRevision {
        let mut hasher = Sha256::new();
        for digest in &self.digests {
            hasher.update(digest);
        }
        UserSettingsRevision {
            hash_algorithm: REVISION_ALGORITHM.into(),
            settings_hash: hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        }
    }
}

/// Legacy fields in the core file take precedence over their section files during migration.
pub(super) fn load_user(
    root: &Path,
    defaults: &DefaultSections,
) -> Result<StoredUserSettings, DomainError> {
    let mut texts = <[Vec<u8>; 5]>::default();
    let core;
    (texts[CORE], core) = read_section(root, CORE, defaults)?;
    let mut sections = UserSettingsSections::split(core)
        .map_err(json_error)?
        .into_files();
    let mut migrated = false;
    for index in 0..CORE {
        if sections[index].is_empty() {
            (texts[index], sections[index]) = read_section(root, index, defaults)?;
        } else {
            texts[index] = sections[index].to_vec();
            persist_changed_blocking(&root.join(SECTION_FILES[index]), &texts[index])?;
            migrated = true;
        }
    }
    if migrated {
        texts[CORE] = sections[CORE].to_vec();
        persist_bytes_file_blocking(&root.join(SECTION_FILES[CORE]), &texts[CORE])?;
    }
    let mut settings = UserSettingsSections::from_files(sections)
        .into_settings()
        .map_err(json_error)?;
    repair(&mut settings);
    Ok(StoredUserSettings {
        settings: UserSettings {
            data: serde_json::to_value(&settings).map_err(json_error)?,
        },
        revision: StoredSections::new(texts).revision(),
    })
}

pub(super) fn write_user(
    root: &Path,
    mut settings: JsonDocument,
    mut stored: StoredSections,
) -> Result<UserSettingsRevision, DomainError> {
    repair(&mut settings);
    let sections = UserSettingsSections::split(settings)
        .map_err(json_error)?
        .into_files();
    for (index, section) in sections.iter().enumerate() {
        let text = section.to_vec();
        if text != stored.texts[index] {
            persist_bytes_file_blocking(&root.join(SECTION_FILES[index]), &text)?;
            stored.digests[index] = Sha256::digest(&text).into();
            stored.texts[index] = text;
        }
    }
    Ok(stored.revision())
}

pub(super) fn save_user(
    root: &Path,
    settings: UserSettings,
) -> Result<UserSettingsRevision, DomainError> {
    let settings = JsonDocument::from_serialize(&settings.data).map_err(json_error)?;
    write_user(root, settings, StoredSections::read(root)?)
}

pub(super) fn reset_user(
    root: &Path,
    defaults: &DefaultSections,
    disabled_extensions: &[String],
) -> Result<(), DomainError> {
    let mut sections = UserSettingsSections::from_files(
        defaults
            .0
            .each_ref()
            .map(|text| JsonDocument::parse(text).expect("default sections are JSON objects")),
    );
    let extension_settings = sections
        .core
        .object_entry(&["extension_settings"])
        .map_err(json_error)?;
    let mut disabled = match extension_settings
        .get(&["disabledExtensions"])
        .map_err(json_error)?
    {
        Some(Value::Array(names)) => names,
        _ => Vec::new(),
    };
    for name in disabled_extensions {
        let name = Value::String(name.clone());
        if !disabled.contains(&name) {
            disabled.push(name);
        }
    }
    extension_settings.set("disabledExtensions", &Value::Array(disabled));
    sections.layout.set("firstRun", &Value::Bool(false));

    for (name, section) in SECTION_FILES.into_iter().zip(sections.into_files()) {
        if name != PERSONA_STATE_FILE {
            persist_changed_blocking(&root.join(name), &section.to_vec())?;
        }
    }
    Ok(())
}

pub(super) fn take_personas(settings: &mut JsonDocument) -> Result<Option<Personas>, DomainError> {
    let Some(power_user) = settings.object_mut(&["power_user"]).map_err(json_error)? else {
        return Ok(None);
    };
    take_personas_with(|field| {
        let value = power_user.get(&[field]).map_err(|error| {
            DomainError::InvalidData(format!("Invalid settings power_user.{field}: {error}"))
        })?;
        power_user.remove(field);
        Ok(value)
    })
}

/// Optional repair must leave undecodable lists intact.
fn repair(settings: &mut JsonDocument) {
    let mut report = UserSettingsRepairReport::default();
    repair_prompt_lists(settings, &mut report);
    match settings.object_mut(&[PROMPT_MANAGER_SETTINGS]) {
        Ok(Some(prompt_manager)) => repair_prompt_lists(prompt_manager, &mut report),
        Ok(None) => {}
        Err(error) => tracing::warn!("Leaving {PROMPT_MANAGER_SETTINGS} unrepaired: {error}"),
    }
    if report.changed() {
        tracing::warn!("Repaired SillyTavern PromptManager settings: {report}");
    }
}

fn repair_prompt_lists(object: &mut JsonDocument, report: &mut UserSettingsRepairReport) {
    let mut lists = Map::new();
    for field in PROMPT_MANAGER_LISTS {
        match object.get(&[field]) {
            Ok(Some(list)) => {
                lists.insert(field.to_owned(), list);
            }
            Ok(None) => {}
            Err(error) => tracing::warn!("Leaving PromptManager {field} unrepaired: {error}"),
        }
    }
    let before = *report;
    repair_prompt_manager_object(&mut lists, report);
    if *report != before {
        for (field, value) in &lists {
            object.set(field, value);
        }
    }
}

/// Preserve corrupt section files before restoring their defaults.
fn read_section(
    root: &Path,
    index: usize,
    defaults: &DefaultSections,
) -> Result<(Vec<u8>, JsonDocument), DomainError> {
    let path = root.join(SECTION_FILES[index]);
    let default_text = &defaults.0[index];
    match fs::read(&path) {
        Ok(stored) => match JsonDocument::parse(&stored) {
            Ok(section) => return Ok((stored, section)),
            Err(error) => replace_corrupt(
                &path,
                &format!("Invalid settings {}: {error}", path.display()),
                default_text,
            )?,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            persist_bytes_file_blocking(&path, default_text)?
        }
        Err(error) => return Err(read_error(&path, error)),
    }
    let text = default_text.clone();
    let section = JsonDocument::parse(&text).expect("default sections are JSON objects");
    Ok((text, section))
}

pub(super) fn load_native(
    root: &Path,
    initial_settings: &TauriTavernSettings,
) -> Result<TauriTavernSettings, DomainError> {
    let path = root.join(NATIVE_FILE);
    let mut defaults = serde_json::to_value(initial_settings).map_err(json_error)?;
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
        persist_changed_blocking(&theme_path, &pretty_json(legacy)?)?;
    }
    settings.dynamic_theme = load_or_default(&theme_path, &DynamicThemeSettings::default())?;
    if missing_core || legacy_theme.is_some() {
        persist_json_file_blocking(&path, &value)?;
    }
    Ok(settings)
}

pub(super) fn save_native(root: &Path, settings: &TauriTavernSettings) -> Result<(), DomainError> {
    for (name, text) in native_files(settings)? {
        persist_changed_blocking(&root.join(name), &text)?;
    }
    Ok(())
}

pub(super) fn reset_native(root: &Path, settings: &TauriTavernSettings) -> Result<(), DomainError> {
    for (name, text) in native_files(settings)? {
        let path = root.join(name);
        if let Some(kept) = replace_keeping_original_blocking(&path, KeptOriginal::SafeMode, &text)?
        {
            tracing::info!("Safe mode kept {name} as {}", kept.display());
        }
    }
    Ok(())
}

fn native_files(settings: &TauriTavernSettings) -> Result<[(&str, Vec<u8>); 2], DomainError> {
    let mut core = serde_json::to_value(settings).map_err(json_error)?;
    let theme = core
        .as_object_mut()
        .expect("settings object")
        .remove("dynamic_theme")
        .expect("dynamic theme field");
    Ok([
        (DYNAMIC_THEME_FILE, pretty_json(&theme)?),
        (NATIVE_FILE, pretty_json(&core)?),
    ])
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, DomainError> {
    serde_json::to_vec_pretty(value).map_err(json_error)
}

fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, DomainError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(read_error(path, error)),
    };
    serde_json::from_reader(std::io::BufReader::new(file))
        .map(Some)
        .map_err(|error| {
            DomainError::InvalidData(format!("Invalid settings {}: {error}", path.display()))
        })
}

fn load_or_default<T: DeserializeOwned + Serialize + Clone>(
    path: &Path,
    defaults: &T,
) -> Result<T, DomainError> {
    match read_optional(path) {
        Ok(Some(value)) => return Ok(value),
        Ok(None) => persist_json_file_blocking(path, defaults)?,
        Err(DomainError::InvalidData(error)) => {
            replace_corrupt(path, &error, &pretty_json(defaults)?)?
        }
        Err(error) => return Err(error),
    }
    Ok(defaults.clone())
}

fn replace_corrupt(path: &Path, error: &str, defaults: &[u8]) -> Result<(), DomainError> {
    if let Some(kept) = replace_keeping_original_blocking(path, KeptOriginal::Corrupt, defaults)? {
        tracing::error!(
            target: tt_contracts::observability::USER_VISIBLE_ERROR,
            "{error}; original saved at {}; restoring this settings section to defaults",
            kept.display(),
        );
    }
    Ok(())
}

fn read_error(path: &Path, error: std::io::Error) -> DomainError {
    DomainError::InternalError(format!(
        "Failed to read settings {}: {error}",
        path.display()
    ))
}

fn json_error(error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid settings: {error}"))
}
