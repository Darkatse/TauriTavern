use chrono::{NaiveDateTime, SecondsFormat, Utc};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::repositories::settings_repository::SettingsRepository;
use crate::infrastructure::persistence::file_system::{
    DataDirectory, list_files_with_extension, replace_file_with_fallback, unique_temp_path,
};
use crate::infrastructure::persistence::png_utils::{
    read_character_data_from_png, write_character_data_to_png,
};
use crate::infrastructure::repositories::file_settings_repository::FileSettingsRepository;

fn is_non_character_png_error(error: &DomainError) -> bool {
    let DomainError::InvalidData(message) = error else {
        return false;
    };

    message == "PNG metadata does not contain any text chunks"
        || message == "PNG metadata does not contain character data"
}

fn migrate_legacy_character_create_date_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S UTC") else {
        return None;
    };

    Some(
        chrono::DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

/// DEPRECATED(MIGRATION): One-time startup migration for legacy character cards that were persisted
/// with a non-ISO `create_date` value (e.g. `2026-03-16 12:34:56 UTC`), which breaks upstream
/// SillyTavern "Newest" sorting and extensions that parse `create_date`.
///
/// Remove after the migration window once older client versions are no longer supported.
#[deprecated(note = "Legacy startup migration. Remove after migration window.")]
pub async fn migrate_legacy_character_create_date_once(
    data_directory: &DataDirectory,
) -> Result<(), DomainError> {
    let settings_repository = FileSettingsRepository::new(data_directory.settings().to_path_buf());
    let mut settings = settings_repository.load_tauritavern_settings().await?;

    if settings.migrations.character_create_date_iso_v1 {
        return Ok(());
    }

    let character_files = list_files_with_extension(data_directory.characters(), "png").await?;
    tracing::info!(
        "Running deprecated migration: character create_date ISO normalization (v1) on {} file(s)",
        character_files.len()
    );

    let mut migrated = 0usize;

    for path in character_files {
        let file_data = fs::read(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read character file '{}': {}",
                path.display(),
                error
            ))
        })?;

        let card_json = match read_character_data_from_png(&file_data) {
            Ok(payload) => payload,
            Err(error) if is_non_character_png_error(&error) => continue,
            Err(error) => return Err(error),
        };

        let mut value: serde_json::Value = serde_json::from_str(&card_json).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to parse character payload JSON in '{}': {}",
                path.display(),
                error
            ))
        })?;

        let Some(object) = value.as_object_mut() else {
            continue;
        };

        let Some(create_date) = object.get("create_date").and_then(|value| value.as_str()) else {
            continue;
        };

        let Some(migrated_value) = migrate_legacy_character_create_date_value(create_date) else {
            continue;
        };

        if migrated_value == create_date {
            continue;
        }

        object.insert(
            "create_date".to_string(),
            serde_json::Value::String(migrated_value),
        );

        let updated_json = serde_json::to_string(&value).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize migrated character payload for '{}': {}",
                path.display(),
                error
            ))
        })?;

        let updated_png = write_character_data_to_png(&file_data, &updated_json)?;

        let temp_path = unique_temp_path(&path, "character.png");
        fs::write(&temp_path, updated_png).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write migrated character temp file '{}': {}",
                temp_path.display(),
                error
            ))
        })?;
        replace_file_with_fallback(&temp_path, &path).await?;

        migrated += 1;
    }

    settings.migrations.character_create_date_iso_v1 = true;
    settings_repository.save_tauritavern_settings(&settings).await?;

    tracing::info!(
        "Deprecated migration completed: character create_date ISO normalization (v1). Updated {} file(s).",
        migrated
    );

    Ok(())
}

