use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::presentation::commands::chunk_body::chunk_bytes_from_request;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;

const STAGING_ROOT_NAME: &str = "tauritavern-upload-staging";
const DEFAULT_KIND: &str = "generic";
const DATA_ARCHIVE_KIND: &str = "data-archive";
const HEADER_FILE_PATH: &str = "file-path";
const HEADER_OFFSET: &str = "offset";
const MOBILE_SMALL_ASSET_CHUNK_BYTES: u64 = 512 * 1024;
const MOBILE_DEFAULT_CHUNK_BYTES: u64 = 1024 * 1024;
const DESKTOP_DEFAULT_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct StageUploadBeginDto {
    pub kind: Option<String>,
    pub preferred_extension: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct StageUploadBeginResult {
    pub file_path: String,
    pub chunk_size: u64,
}

#[derive(Debug, Serialize)]
pub struct StageUploadFinishResult {
    pub file_path: String,
    pub size: u64,
}

fn normalize_kind(value: Option<&str>) -> Result<String, CommandError> {
    let kind = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_KIND);

    if kind.len() > 48
        || !kind
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(CommandError::BadRequest("Invalid upload kind".to_string()));
    }

    Ok(kind.to_string())
}

fn normalize_extension(value: Option<&str>) -> Result<String, CommandError> {
    let extension = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("bin")
        .trim_start_matches('.')
        .to_ascii_lowercase();

    if extension.is_empty()
        || extension.len() > 12
        || !extension.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        return Err(CommandError::BadRequest(
            "Invalid upload extension".to_string(),
        ));
    }

    Ok(extension)
}

fn chunk_size_for_kind(kind: &str) -> u64 {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return match kind {
            "avatar" | "user-avatar" | "worldinfo-import" => MOBILE_SMALL_ASSET_CHUNK_BYTES,
            _ => MOBILE_DEFAULT_CHUNK_BYTES,
        };
    }

    DESKTOP_DEFAULT_CHUNK_BYTES
}

fn staging_root(app: &AppHandle, kind: &str) -> Result<PathBuf, CommandError> {
    let path_resolver = app.path();
    let base = path_resolver
        .app_cache_dir()
        .or_else(|_| path_resolver.temp_dir())
        .map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to resolve upload staging directory: {}",
                error
            ))
        })?;

    Ok(base.join(STAGING_ROOT_NAME).join(kind))
}

fn ensure_mobile_archive_uses_native_picker(kind: &str) -> Result<(), CommandError> {
    if cfg!(any(target_os = "android", target_os = "ios")) && kind == DATA_ARCHIVE_KIND {
        return Err(CommandError::BadRequest(
            "Mobile data archive imports must use the native archive picker".to_string(),
        ));
    }

    Ok(())
}

fn validate_staged_path(app: &AppHandle, file_path: &str) -> Result<PathBuf, CommandError> {
    let requested = PathBuf::from(file_path.trim());
    if !requested.is_absolute() {
        return Err(CommandError::BadRequest(
            "Upload staging path must be absolute".to_string(),
        ));
    }

    let parent = requested.parent().ok_or_else(|| {
        CommandError::BadRequest("Upload staging path is missing a parent directory".to_string())
    })?;
    let root = staging_root(app, DEFAULT_KIND)?
        .parent()
        .ok_or_else(|| {
            CommandError::InternalServerError("Invalid upload staging root".to_string())
        })?
        .to_path_buf();

    let canonical_root = canonicalize_existing(&root, "upload staging root")?;
    let canonical_parent = canonicalize_existing(parent, "upload staging parent")?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(CommandError::BadRequest(
            "Upload staging path is outside the staging directory".to_string(),
        ));
    }

    Ok(requested)
}

fn canonicalize_existing(path: &Path, label: &str) -> Result<PathBuf, CommandError> {
    std::fs::canonicalize(path).map_err(|error| {
        CommandError::InternalServerError(format!("Failed to canonicalize {}: {}", label, error))
    })
}

fn required_header(request: &tauri::ipc::Request<'_>, name: &str) -> Result<String, CommandError> {
    request
        .headers()
        .get(name)
        .ok_or_else(|| CommandError::BadRequest(format!("Missing upload staging header: {name}")))?
        .to_str()
        .map(str::to_string)
        .map_err(|_| CommandError::BadRequest(format!("Invalid upload staging header: {name}")))
}

fn chunk_file_path_from_request(request: &tauri::ipc::Request<'_>) -> Result<String, CommandError> {
    let encoded = required_header(request, HEADER_FILE_PATH)?;
    percent_decode_str(&encoded)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| {
            CommandError::BadRequest("Upload staging file path header is invalid".to_string())
        })
}

fn chunk_offset_from_request(request: &tauri::ipc::Request<'_>) -> Result<u64, CommandError> {
    let offset = required_header(request, HEADER_OFFSET)?;
    offset.parse::<u64>().map_err(|_| {
        CommandError::BadRequest("Upload staging offset header is invalid".to_string())
    })
}

#[tauri::command]
pub async fn stage_upload_begin(
    app: AppHandle,
    dto: StageUploadBeginDto,
) -> Result<StageUploadBeginResult, CommandError> {
    let kind = normalize_kind(dto.kind.as_deref())?;
    ensure_mobile_archive_uses_native_picker(&kind)?;
    let extension = normalize_extension(dto.preferred_extension.as_deref())?;
    log_command(format!(
        "stage_upload_begin kind={} size={}",
        kind,
        dto.size.unwrap_or(0)
    ));

    let directory = staging_root(&app, &kind)?;
    fs::create_dir_all(&directory).await.map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to create upload staging directory: {}",
            error
        ))
    })?;

    let file_name = format!("{}.{}", uuid::Uuid::new_v4().simple(), extension);
    let file_path = directory.join(file_name);
    fs::File::create(&file_path).await.map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to create upload staging file: {}",
            error
        ))
    })?;

    Ok(StageUploadBeginResult {
        file_path: file_path.to_string_lossy().to_string(),
        chunk_size: chunk_size_for_kind(&kind),
    })
}

#[tauri::command]
pub async fn stage_upload_chunk(
    app: AppHandle,
    request: tauri::ipc::Request<'_>,
) -> Result<u64, CommandError> {
    let file_path = chunk_file_path_from_request(&request)?;
    let offset = chunk_offset_from_request(&request)?;
    let data = chunk_bytes_from_request(&request)?;
    let path = validate_staged_path(&app, &file_path)?;
    let metadata = fs::metadata(&path).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to stat upload staging file: {}", error))
    })?;
    let current_len = metadata.len();
    if current_len != offset {
        return Err(CommandError::BadRequest(format!(
            "Upload staging offset mismatch: expected {}, got {}",
            current_len, offset
        )));
    }

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .await
        .map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to open upload staging file: {}",
                error
            ))
        })?;

    file.write_all(&data).await.map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to write upload staging chunk: {}",
            error
        ))
    })?;

    Ok(offset + data.len() as u64)
}

#[tauri::command]
pub async fn stage_upload_finish(
    app: AppHandle,
    file_path: String,
    expected_size: u64,
) -> Result<StageUploadFinishResult, CommandError> {
    let path = validate_staged_path(&app, &file_path)?;
    let metadata = fs::metadata(&path).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to stat upload staging file: {}", error))
    })?;
    let size = metadata.len();
    if size != expected_size {
        return Err(CommandError::BadRequest(format!(
            "Upload staging size mismatch: expected {}, got {}",
            expected_size, size
        )));
    }

    Ok(StageUploadFinishResult {
        file_path: path.to_string_lossy().to_string(),
        size,
    })
}

#[tauri::command]
pub async fn stage_upload_discard(app: AppHandle, file_path: String) -> Result<(), CommandError> {
    let path = validate_staged_path(&app, &file_path)?;
    match fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CommandError::InternalServerError(format!(
            "Failed to remove upload staging file: {}",
            error
        ))),
    }
}
