use std::fs;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::fields::{
    DYNAMIC_THEME_FILE, NATIVE_FILE, PERSONA_STATE_FILE, SECTION_FILES, UserSettingsSections,
};
use super::lazy_object::{LazyObject, Member};
use crate::file_system::{
    KeptOriginal, persist_bytes_file_blocking, persist_json_file_blocking,
    replace_keeping_original_blocking,
};
use tt_domain::errors::DomainError;
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

/// The bundled defaults as section texts, split once.
pub(super) struct DefaultSections([Vec<u8>; 5]);

impl DefaultSections {
    pub(super) fn new(defaults: &UserSettings) -> Self {
        let settings = LazyObject::from_value(&defaults.data)
            .expect("bundled default settings are a JSON object");
        Self(
            UserSettingsSections::split(settings)
                .into_files()
                .map(|section| section.to_bytes()),
        )
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

/// Reads the sections, normalizing without writing; a core that still holds fields of other sections,
/// from an older version or an import, has them moved into their files first.
pub(super) fn load_user(
    root: &Path,
    defaults: &DefaultSections,
) -> Result<StoredUserSettings, DomainError> {
    let mut texts = <[Vec<u8>; 5]>::default();
    let core;
    (texts[CORE], core) = read_section(root, CORE, defaults)?;
    let mut sections = UserSettingsSections::split(core).into_files();
    let mut migrated = false;
    for index in 0..CORE {
        if sections[index].is_empty() {
            (texts[index], sections[index]) = read_section(root, index, defaults)?;
        } else {
            texts[index] = sections[index].to_bytes();
            persist_changed(&root.join(SECTION_FILES[index]), &texts[index])?;
            migrated = true;
        }
    }
    if migrated {
        texts[CORE] = sections[CORE].to_bytes();
        persist_bytes_file_blocking(&root.join(SECTION_FILES[CORE]), &texts[CORE])?;
    }
    let mut settings = UserSettingsSections::from_files(sections).into_settings();
    repair(&mut settings);
    Ok(StoredUserSettings {
        settings: UserSettings {
            data: settings.into_value(),
        },
        revision: StoredSections::new(texts).revision(),
    })
}

/// Normalizes the settings once, then writes only the sections whose text changed.
pub(super) fn write_user(
    root: &Path,
    mut settings: LazyObject,
    mut stored: StoredSections,
) -> Result<UserSettingsRevision, DomainError> {
    repair(&mut settings);
    let sections = UserSettingsSections::split(settings).into_files();
    for (index, section) in sections.iter().enumerate() {
        let text = section.to_bytes();
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
    let settings = LazyObject::from_value(&settings.data).map_err(json_error)?;
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
            .map(|text| LazyObject::parse(text).expect("default sections are JSON objects")),
    );
    let extension_settings = sections.core.object_entry(&["extension_settings"]);
    let mut disabled = match extension_settings.value("disabledExtensions") {
        Some(Value::Array(names)) => names,
        _ => Vec::new(),
    };
    for name in disabled_extensions {
        let name = Value::String(name.clone());
        if !disabled.contains(&name) {
            disabled.push(name);
        }
    }
    extension_settings.set_value("disabledExtensions", &Value::Array(disabled));
    sections.layout.set_value("firstRun", &Value::Bool(false));

    for (name, section) in SECTION_FILES.into_iter().zip(sections.into_files()) {
        if name != PERSONA_STATE_FILE {
            persist_changed(&root.join(name), &section.to_bytes())?;
        }
    }
    Ok(())
}

/// Takes the Persona registry projection out of `power_user`.
pub(super) fn take_personas(settings: &mut LazyObject) -> Result<Option<Personas>, DomainError> {
    let Some(power_user) = settings.object_mut(&["power_user"]) else {
        return Ok(None);
    };
    take_personas_with(|field| power_user.remove(field).map(Member::into_value))
}

fn repair(settings: &mut LazyObject) {
    let mut report = UserSettingsRepairReport::default();
    repair_prompt_lists(settings, &mut report);
    if let Some(prompt_manager) = settings.object_mut(&[PROMPT_MANAGER_SETTINGS]) {
        repair_prompt_lists(prompt_manager, &mut report);
    }
    if report.changed() {
        tracing::warn!("Repaired SillyTavern PromptManager settings: {report}");
    }
}

/// Opens only the PromptManager lists, and replaces them only when the repair changed them.
fn repair_prompt_lists(object: &mut LazyObject, report: &mut UserSettingsRepairReport) {
    let mut lists: Map<String, Value> = PROMPT_MANAGER_LISTS
        .iter()
        .filter_map(|field| Some(((*field).to_owned(), object.value(field)?)))
        .collect();
    let before = *report;
    repair_prompt_manager_object(&mut lists, report);
    if *report != before {
        for (field, value) in &lists {
            object.set_value(field, value);
        }
    }
}

/// A section file as stored. A missing one gets the default; one that is not a JSON object is kept
/// beside it as corrupt, and its section restarts from the default.
fn read_section(
    root: &Path,
    index: usize,
    defaults: &DefaultSections,
) -> Result<(Vec<u8>, LazyObject), DomainError> {
    let path = root.join(SECTION_FILES[index]);
    let default_text = &defaults.0[index];
    match fs::read(&path) {
        Ok(stored) => match LazyObject::parse(&stored) {
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
    let section = LazyObject::parse(&text).expect("default sections are JSON objects");
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
        persist_changed(&theme_path, &pretty_json(legacy)?)?;
    }
    settings.dynamic_theme = load_or_default(&theme_path, &DynamicThemeSettings::default())?;
    if missing_core || legacy_theme.is_some() {
        persist_json_file_blocking(&path, &value)?;
    }
    Ok(settings)
}

pub(super) fn save_native(root: &Path, settings: &TauriTavernSettings) -> Result<(), DomainError> {
    for (name, text) in native_files(settings)? {
        persist_changed(&root.join(name), &text)?;
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

/// Rewrites a file only when the bytes to write differ from the stored ones.
fn persist_changed(path: &Path, text: &[u8]) -> Result<(), DomainError> {
    match fs::read(path) {
        Ok(stored) if stored == text => Ok(()),
        Ok(_) => persist_bytes_file_blocking(path, text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            persist_bytes_file_blocking(path, text)
        }
        Err(error) => Err(read_error(path, error)),
    }
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
