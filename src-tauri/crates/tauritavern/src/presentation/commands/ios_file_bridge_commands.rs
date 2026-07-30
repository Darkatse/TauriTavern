#![cfg(target_os = "ios")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::app::AppState;
use crate::platform::ios_document_picker::{
    PickDocumentResult, copy_picked_url_to_path, pick_character_card, pick_data_archive,
    pick_skill_import_archive,
};
use crate::platform::ios_share_sheet::share_file;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_contracts::host::IOS_EXPORT_STAGING_ROOT_NAME;
use tt_domain::errors::DomainError;

const IOS_SKILL_IMPORT_STAGING_ROOT_NAME: &str = "tauritavern-skill-import-staging";
const IOS_CHARACTER_IMPORT_STAGING_ROOT_NAME: &str = "tauritavern-character-import-staging";
const CHARACTER_CARD_EXTENSIONS: &[&str] = &["json", "png"];
const CHARACTER_IMPORT_STAGING_FILE_PREFIX: &str = "tauritavern-character-import-";
const CHARACTER_IMPORT_STAGING_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct IosImportArchiveResponse {
    pub cancelled: bool,
    pub job_id: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IosPickSkillImportArchiveResponse {
    pub cancelled: bool,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IosPickCharacterCardResponse {
    pub cancelled: bool,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IosShareExportArchiveResponse {
    pub completed: bool,
    pub activity: Option<String>,
    pub cleanup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IosShareFileResponse {
    pub completed: bool,
    pub activity: Option<String>,
}

#[tauri::command]
pub async fn ios_import_data_archive_from_picker(
    app_state: State<'_, Arc<AppState>>,
    window: WebviewWindow,
) -> Result<IosImportArchiveResponse, CommandError> {
    log_command("ios_import_data_archive_from_picker");

    let picked = match pick_data_archive(&window)
        .await
        .map_err(map_command_error("Failed to present iOS document picker"))?
    {
        PickDocumentResult::Cancelled => {
            return Ok(IosImportArchiveResponse {
                cancelled: true,
                job_id: None,
                file_name: None,
            });
        }
        PickDocumentResult::Picked(picked) => picked,
    };

    let data_archive_service = app_state.services.data_archive_service.clone();
    let picked_url = picked.url.clone();
    let picked_file_name = picked.file_name.clone();

    let target_path =
        tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, DomainError> {
            let target_path = data_archive_service.prepare_incoming_import_archive_path()?;
            if let Err(error) = copy_picked_url_to_path(&picked_url, &target_path) {
                let _ = fs::remove_file(&target_path);
                return Err(error);
            }
            Ok(target_path)
        })
        .await
        .map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to join iOS import staging task: {}",
                error
            ))
        })?
        .map_err(map_command_error("Failed to stage iOS data archive import"))?;

    let _cleanup_target = CleanupTempFile::new(target_path.clone());
    let job_id = app_state
        .services
        .data_archive_service
        .start_import(&target_path, true)
        .map_err(map_command_error("Failed to start data archive import"))?;

    Ok(IosImportArchiveResponse {
        cancelled: false,
        job_id: Some(job_id),
        file_name: Some(picked_file_name),
    })
}

struct CleanupTempFile {
    path: PathBuf,
}

impl CleanupTempFile {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for CleanupTempFile {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to cleanup temporary file {}: {}",
                    self.path.display(),
                    error
                );
            }
        }
    }
}

fn prepare_skill_import_archive_path(app: &AppHandle) -> Result<PathBuf, DomainError> {
    let path_resolver = app.path();
    let staging_root = match path_resolver.app_cache_dir() {
        Ok(path) => path.join(IOS_SKILL_IMPORT_STAGING_ROOT_NAME),
        Err(cache_error) => path_resolver
            .temp_dir()
            .map(|path| path.join(IOS_SKILL_IMPORT_STAGING_ROOT_NAME))
            .map_err(|temp_error| {
                DomainError::InternalError(format!(
                    "Failed to resolve iOS Skill import staging directory. app cache: {}; temp: {}",
                    cache_error, temp_error
                ))
            })?,
    };

    fs::create_dir_all(&staging_root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create iOS Skill import staging directory {}: {}",
            staging_root.display(),
            error
        ))
    })?;

    Ok(staging_root.join(format!(
        "tauritavern-skill-import-{}.zip",
        uuid::Uuid::new_v4().simple()
    )))
}

fn prepare_character_import_file_path(
    app: &AppHandle,
    picked_file_name: &str,
) -> Result<PathBuf, DomainError> {
    let extension = Path::new(picked_file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_lowercase())
        .filter(|value| CHARACTER_CARD_EXTENSIONS.contains(&value.as_str()))
        .ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Unsupported character card file extension: {}",
                picked_file_name
            ))
        })?;

    let path_resolver = app.path();
    let staging_root = match path_resolver.app_cache_dir() {
        Ok(path) => path.join(IOS_CHARACTER_IMPORT_STAGING_ROOT_NAME),
        Err(cache_error) => path_resolver
            .temp_dir()
            .map(|path| path.join(IOS_CHARACTER_IMPORT_STAGING_ROOT_NAME))
            .map_err(|temp_error| {
                DomainError::InternalError(format!(
                    "Failed to resolve iOS character import staging directory. app cache: {}; temp: {}",
                    cache_error, temp_error
                ))
            })?,
    };

    fs::create_dir_all(&staging_root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create iOS character import staging directory {}: {}",
            staging_root.display(),
            error
        ))
    })?;

    cleanup_stale_character_import_files(&staging_root);

    Ok(staging_root.join(format!(
        "{}{}.{}",
        CHARACTER_IMPORT_STAGING_FILE_PREFIX,
        uuid::Uuid::new_v4().simple(),
        extension
    )))
}

fn cleanup_stale_character_import_files(staging_root: &Path) {
    let entries = match fs::read_dir(staging_root) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(
                "Failed to read iOS character import staging directory {}: {}",
                staging_root.display(),
                error
            );
            return;
        }
    };
    let now = SystemTime::now();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    "Failed to read an iOS character import staging entry in {}: {}",
                    staging_root.display(),
                    error
                );
                continue;
            }
        };
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with(CHARACTER_IMPORT_STAGING_FILE_PREFIX) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::warn!(
                    "Failed to read iOS character import staging file metadata {}: {}",
                    path.display(),
                    error
                );
                continue;
            }
        };
        if !metadata.is_file() {
            continue;
        }

        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= CHARACTER_IMPORT_STAGING_TTL);
        if !stale {
            continue;
        }

        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to cleanup stale iOS character import staging file {}: {}",
                    path.display(),
                    error
                );
            }
        }
    }
}

#[tauri::command]
pub async fn ios_pick_skill_import_archive(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<IosPickSkillImportArchiveResponse, CommandError> {
    log_command("ios_pick_skill_import_archive");

    let picked = match pick_skill_import_archive(&window)
        .await
        .map_err(map_command_error(
            "Failed to present iOS Skill import picker",
        ))? {
        PickDocumentResult::Cancelled => {
            return Ok(IosPickSkillImportArchiveResponse {
                cancelled: true,
                file_path: None,
                file_name: None,
            });
        }
        PickDocumentResult::Picked(picked) => picked,
    };

    let app_handle = app.clone();
    let picked_url = picked.url.clone();
    let picked_file_name = picked.file_name.clone();

    let target_path =
        tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, DomainError> {
            let target_path = prepare_skill_import_archive_path(&app_handle)?;
            match copy_picked_url_to_path(&picked_url, &target_path) {
                Ok(()) => Ok(target_path),
                Err(error) => {
                    let _ = fs::remove_file(&target_path);
                    Err(error)
                }
            }
        })
        .await
        .map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to join iOS Skill import staging task: {}",
                error
            ))
        })?
        .map_err(map_command_error(
            "Failed to stage iOS Skill import archive",
        ))?;

    Ok(IosPickSkillImportArchiveResponse {
        cancelled: false,
        file_path: Some(target_path.to_string_lossy().to_string()),
        file_name: Some(picked_file_name),
    })
}

#[tauri::command]
pub async fn ios_pick_character_card(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<IosPickCharacterCardResponse, CommandError> {
    log_command("ios_pick_character_card");

    let picked = match pick_character_card(&window)
        .await
        .map_err(map_command_error(
            "Failed to present iOS character card picker",
        ))? {
        PickDocumentResult::Cancelled => {
            return Ok(IosPickCharacterCardResponse {
                cancelled: true,
                file_path: None,
                file_name: None,
            });
        }
        PickDocumentResult::Picked(picked) => picked,
    };

    let app_handle = app.clone();
    let picked_url = picked.url.clone();
    let picked_file_name = picked.file_name.clone();

    let target_path =
        tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, DomainError> {
            let target_path = prepare_character_import_file_path(&app_handle, &picked_file_name)?;
            match copy_picked_url_to_path(&picked_url, &target_path) {
                Ok(()) => Ok(target_path),
                Err(error) => {
                    let _ = fs::remove_file(&target_path);
                    Err(error)
                }
            }
        })
        .await
        .map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to join iOS character import staging task: {}",
                error
            ))
        })?
        .map_err(map_command_error(
            "Failed to stage iOS character card import",
        ))?;

    Ok(IosPickCharacterCardResponse {
        cancelled: false,
        file_path: Some(target_path.to_string_lossy().to_string()),
        file_name: Some(picked.file_name),
    })
}

async fn present_ios_share_sheet_for_path(
    window: &WebviewWindow,
    file_path: &std::path::Path,
) -> Result<IosShareFileResponse, CommandError> {
    let share_result = share_file(window, file_path)
        .await
        .map_err(map_command_error("Failed to present iOS share sheet"))?;

    Ok(IosShareFileResponse {
        completed: share_result.completed,
        activity: share_result.activity,
    })
}

fn resolve_allowed_ios_share_roots(window: &WebviewWindow) -> Result<Vec<PathBuf>, CommandError> {
    let path_resolver = window.app_handle().path();
    let candidate_roots = [
        path_resolver
            .app_cache_dir()
            .map(|path| path.join(IOS_EXPORT_STAGING_ROOT_NAME))
            .map_err(|error| {
                CommandError::InternalServerError(format!(
                    "Failed to resolve iOS app cache directory: {}",
                    error
                ))
            })?,
        path_resolver
            .temp_dir()
            .map(|path| path.join(IOS_EXPORT_STAGING_ROOT_NAME))
            .map_err(|error| {
                CommandError::InternalServerError(format!(
                    "Failed to resolve iOS temp directory: {}",
                    error
                ))
            })?,
    ];

    let mut allowed_roots = Vec::new();
    for root in candidate_roots {
        match fs::canonicalize(&root) {
            Ok(path) => allowed_roots.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CommandError::InternalServerError(format!(
                    "Failed to resolve iOS share staging root {}: {}",
                    root.display(),
                    error
                )));
            }
        }
    }

    if allowed_roots.is_empty() {
        return Err(CommandError::InternalServerError(
            "No iOS share staging root is available".to_string(),
        ));
    }

    Ok(allowed_roots)
}

fn resolve_ios_shareable_file_path(
    window: &WebviewWindow,
    file_path: &str,
) -> Result<PathBuf, CommandError> {
    let requested_path = PathBuf::from(file_path);
    if !requested_path.is_absolute() {
        return Err(CommandError::BadRequest(
            "iOS share path must be absolute".to_string(),
        ));
    }

    let canonical_file_path = fs::canonicalize(&requested_path)
        .map_err(|_| CommandError::BadRequest("Invalid iOS share path".to_string()))?;
    let allowed_roots = resolve_allowed_ios_share_roots(window)?;

    if allowed_roots
        .iter()
        .any(|root| canonical_file_path.starts_with(root))
    {
        return Ok(canonical_file_path);
    }

    Err(CommandError::BadRequest(format!(
        "iOS share path must be inside {}",
        Path::new(IOS_EXPORT_STAGING_ROOT_NAME).display()
    )))
}

#[tauri::command]
pub async fn ios_share_file(
    window: WebviewWindow,
    file_path: String,
) -> Result<IosShareFileResponse, CommandError> {
    log_command("ios_share_file");

    let file_path = file_path.trim();
    if file_path.is_empty() {
        return Err(CommandError::BadRequest("Missing file_path".to_string()));
    }

    let file_path = resolve_ios_shareable_file_path(&window, file_path)?;
    present_ios_share_sheet_for_path(&window, &file_path).await
}

#[tauri::command]
pub async fn ios_share_export_data_archive(
    app_state: State<'_, Arc<AppState>>,
    window: WebviewWindow,
    job_id: String,
) -> Result<IosShareExportArchiveResponse, CommandError> {
    log_command("ios_share_export_data_archive");

    let job_id = job_id.trim().to_string();
    if job_id.is_empty() {
        return Err(CommandError::BadRequest("Missing job_id".to_string()));
    }

    let archive_path = app_state
        .services
        .data_archive_service
        .completed_export_archive_path(&job_id)
        .map_err(map_command_error("Failed to resolve export archive path"))?;

    let share_result = present_ios_share_sheet_for_path(&window, &archive_path).await?;

    let cleanup_error = match app_state
        .services
        .data_archive_service
        .finalize_export_delivery(&job_id, None)
    {
        Ok(cleanup_error) => cleanup_error,
        Err(error) => Some(error.to_string()),
    };

    Ok(IosShareExportArchiveResponse {
        completed: share_result.completed,
        activity: share_result.activity,
        cleanup_error,
    })
}
