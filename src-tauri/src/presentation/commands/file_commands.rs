use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;
use tauri::State;
use tokio::fs;
use url::Url;

use crate::app::AppState;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;

const UNSAFE_EXTENSIONS: &[&str] = &[
    ".php",
    ".exe",
    ".com",
    ".dll",
    ".pif",
    ".application",
    ".gadget",
    ".msi",
    ".jar",
    ".cmd",
    ".bat",
    ".reg",
    ".sh",
    ".py",
    ".js",
    ".jse",
    ".jsp",
    ".pdf",
    ".html",
    ".htm",
    ".hta",
    ".vb",
    ".vbs",
    ".vbe",
    ".cpl",
    ".msc",
    ".scr",
    ".sql",
    ".iso",
    ".img",
    ".dmg",
    ".ps1",
    ".ps1xml",
    ".ps2",
    ".ps2xml",
    ".psc1",
    ".psc2",
    ".msh",
    ".msh1",
    ".msh2",
    ".mshxml",
    ".msh1xml",
    ".msh2xml",
    ".scf",
    ".lnk",
    ".inf",
    ".doc",
    ".docm",
    ".docx",
    ".dot",
    ".dotm",
    ".dotx",
    ".xls",
    ".xlsm",
    ".xlsx",
    ".xlt",
    ".xltm",
    ".xltx",
    ".xlam",
    ".ppt",
    ".pptm",
    ".pptx",
    ".pot",
    ".potm",
    ".potx",
    ".ppam",
    ".ppsx",
    ".ppsm",
    ".pps",
    ".sldx",
    ".sldm",
    ".ws",
];

#[derive(Debug, Serialize)]
pub struct UserFileUploadResult {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct UserFileAssetPayload {
    pub content_base64: String,
    pub mime_type: String,
}

fn normalize_relative_path(raw: &str) -> Result<PathBuf, CommandError> {
    let normalized = String::from(raw)
        .replace('\\', "/")
        .trim()
        .trim_start_matches('/')
        .to_string();

    if normalized.is_empty() {
        return Err(CommandError::BadRequest(
            "File path cannot be empty".to_string(),
        ));
    }

    let mut path = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(segment) => path.push(segment),
            _ => {
                return Err(CommandError::BadRequest("Invalid file path".to_string()));
            }
        }
    }

    if path.as_os_str().is_empty() {
        return Err(CommandError::BadRequest(
            "File path cannot be empty".to_string(),
        ));
    }

    Ok(path)
}

fn validate_upload_name(raw: &str) -> Result<String, CommandError> {
    let name = String::from(raw).trim().to_string();
    if name.is_empty() {
        return Err(CommandError::BadRequest(
            "No upload name specified".to_string(),
        ));
    }

    if name.starts_with('.') {
        return Err(CommandError::BadRequest(
            "Filename cannot start with '.'".to_string(),
        ));
    }

    if name.contains('/') || name.contains('\\') {
        return Err(CommandError::BadRequest(
            "Illegal character in filename".to_string(),
        ));
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    {
        return Err(CommandError::BadRequest(
            "Illegal character in filename; only alphanumeric, '_', '-' are accepted.".to_string(),
        ));
    }

    let extension = Path::new(&name)
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    if UNSAFE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(CommandError::BadRequest(
            "Forbidden file extension.".to_string(),
        ));
    }

    Ok(name)
}

fn normalize_user_file_reference(raw: &str) -> Result<PathBuf, CommandError> {
    let mut value = String::from(raw).trim().to_string();
    if value.is_empty() {
        return Err(CommandError::BadRequest("No path specified".to_string()));
    }

    if let Ok(parsed_url) = Url::parse(&value) {
        value = parsed_url.path().to_string();
    }

    let normalized = value.replace('\\', "/");
    let without_leading = normalized.trim_start_matches('/');
    let relative = without_leading
        .strip_prefix("user/files/")
        .ok_or_else(|| CommandError::BadRequest("Invalid path".to_string()))?;

    normalize_relative_path(relative)
}

fn resolve_target_path(root: &Path, relative: &Path) -> Result<PathBuf, CommandError> {
    let target = root.join(relative);
    if !target.starts_with(root) {
        return Err(CommandError::BadRequest("Invalid path".to_string()));
    }
    Ok(target)
}

async fn get_default_user_files_directory(
    app_state: &Arc<AppState>,
) -> Result<PathBuf, CommandError> {
    let directory = app_state
        .user_directory_service
        .get_default_user_directory()
        .await?;
    let files_dir = PathBuf::from(directory.files);

    fs::create_dir_all(&files_dir).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to ensure files directory: {}", error))
    })?;

    Ok(files_dir)
}

#[tauri::command]
pub async fn upload_user_file(
    name: String,
    data_base64: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserFileUploadResult, CommandError> {
    log_command(format!("upload_user_file {}", name));

    let validated_name = validate_upload_name(&name)?;
    let bytes = BASE64_STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|error| {
            CommandError::BadRequest(format!("No upload data specified: {}", error))
        })?;

    let files_dir = get_default_user_files_directory(&app_state).await?;
    let target_path = resolve_target_path(&files_dir, Path::new(&validated_name))?;

    fs::write(&target_path, &bytes).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to save file: {}", error))
    })?;

    Ok(UserFileUploadResult {
        path: format!("/user/files/{}", validated_name),
    })
}

#[tauri::command]
pub async fn read_user_file_asset(
    relative_path: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserFileAssetPayload, CommandError> {
    log_command(format!("read_user_file_asset {}", relative_path));

    let relative = normalize_relative_path(&relative_path)?;
    let files_dir = get_default_user_files_directory(&app_state).await?;
    let target_path = resolve_target_path(&files_dir, &relative)?;

    let bytes = fs::read(&target_path)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => CommandError::NotFound("File not found".to_string()),
            _ => CommandError::InternalServerError(format!("Failed to read file: {}", error)),
        })?;

    let mime_type = mime_guess::from_path(&target_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();

    Ok(UserFileAssetPayload {
        content_base64: BASE64_STANDARD.encode(bytes),
        mime_type,
    })
}

#[tauri::command]
pub async fn delete_user_file(
    path: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_user_file {}", path));

    let relative = normalize_user_file_reference(&path)?;
    let files_dir = get_default_user_files_directory(&app_state).await?;
    let target_path = resolve_target_path(&files_dir, &relative)?;

    if !target_path.is_file() {
        return Err(CommandError::NotFound("File not found".to_string()));
    }

    fs::remove_file(&target_path).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to delete file: {}", error))
    })?;

    Ok(())
}

#[tauri::command]
pub async fn verify_user_files(
    urls: Vec<String>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, bool>, CommandError> {
    log_command(format!("verify_user_files {}", urls.len()));

    let files_dir = get_default_user_files_directory(&app_state).await?;
    let mut result = HashMap::with_capacity(urls.len());

    for original_url in urls {
        let Ok(relative) = normalize_user_file_reference(&original_url) else {
            continue;
        };
        let Ok(path) = resolve_target_path(&files_dir, &relative) else {
            continue;
        };
        result.insert(original_url, path.is_file());
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{normalize_relative_path, normalize_user_file_reference, validate_upload_name};

    #[test]
    fn validate_upload_name_accepts_safe_filename() {
        assert_eq!(
            validate_upload_name("LittleWhiteBox_CommonSettings.json").unwrap(),
            "LittleWhiteBox_CommonSettings.json"
        );
    }

    #[test]
    fn validate_upload_name_rejects_unsafe_extension() {
        assert!(validate_upload_name("payload.js").is_err());
    }

    #[test]
    fn normalize_relative_path_rejects_parent_segments() {
        assert!(normalize_relative_path("../secret.txt").is_err());
    }

    #[test]
    fn normalize_user_file_reference_extracts_relative_part() {
        let path = normalize_user_file_reference("user/files/test.json").unwrap();
        assert_eq!(path.to_string_lossy(), "test.json");
    }
}
