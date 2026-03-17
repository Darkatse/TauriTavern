use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;
use tauri::State;
use tokio::fs;

use crate::app::AppState;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;

const MEDIA_EXTENSIONS: &[&str] = &[
    "bmp", "png", "jpg", "webp", "jpeg", "jfif", "gif", "mp4", "avi", "mov", "wmv", "flv", "webm",
    "3gp", "mkv", "mpg", "mp3", "wav", "ogg", "flac", "aac", "m4a", "aiff",
];

const MEDIA_REQUEST_IMAGE: u32 = 0b001;
const MEDIA_REQUEST_VIDEO: u32 = 0b010;
const MEDIA_REQUEST_AUDIO: u32 = 0b100;

#[derive(Debug, Serialize)]
pub struct UserImageUploadResult {
    pub path: String,
}

fn sanitize_filename(filename: &str) -> String {
    let sanitized = filename
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ if character.is_control() => '_',
            _ => character,
        })
        .collect::<String>();

    sanitized.trim().trim_end_matches(['.', ' ']).to_string()
}

fn remove_last_extension(filename: &str) -> &str {
    filename
        .rsplit_once('.')
        .map(|(base, _)| base)
        .unwrap_or(filename)
}

fn to_url_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn client_relative_path(root: &Path, path: &Path) -> Result<String, CommandError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        CommandError::InternalServerError("Failed to compute client relative path".to_string())
    })?;

    Ok(to_url_path(relative))
}

async fn get_default_user_image_directory(
    app_state: &Arc<AppState>,
) -> Result<(PathBuf, PathBuf), CommandError> {
    let directory = app_state
        .user_directory_service
        .get_default_user_directory()
        .await?;
    let root_dir = PathBuf::from(directory.root);
    let images_dir = PathBuf::from(directory.user_images);

    fs::create_dir_all(&images_dir).await.map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to ensure user images directory: {}",
            error
        ))
    })?;

    Ok((root_dir, images_dir))
}

fn validate_media_format(raw: &str) -> Result<String, CommandError> {
    let format = raw.trim().to_ascii_lowercase();
    if format.is_empty() || !MEDIA_EXTENSIONS.contains(&format.as_str()) {
        return Err(CommandError::BadRequest("Invalid image format".to_string()));
    }
    Ok(format)
}

#[tauri::command]
pub async fn upload_user_image(
    image_base64: String,
    format: String,
    filename: Option<String>,
    ch_name: Option<String>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserImageUploadResult, CommandError> {
    log_command("upload_user_image");

    let image_base64 = image_base64.trim().to_string();
    if image_base64.is_empty() {
        return Err(CommandError::BadRequest(
            "No image data provided".to_string(),
        ));
    }

    let format = validate_media_format(&format)?;

    let raw_filename = filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("{}.{}", remove_last_extension(value), format))
        .unwrap_or_else(|| format!("{}.{}", chrono::Utc::now().timestamp_millis(), format));
    let safe_filename = sanitize_filename(&raw_filename);
    if safe_filename.is_empty() {
        return Err(CommandError::BadRequest("Invalid filename".to_string()));
    }

    let safe_folder = ch_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(sanitize_filename)
        .filter(|value| !value.is_empty());

    let (root_dir, images_dir) = get_default_user_image_directory(&app_state).await?;
    let target_path = match safe_folder {
        Some(folder) => images_dir.join(folder).join(&safe_filename),
        None => images_dir.join(&safe_filename),
    };

    let Some(parent) = target_path.parent() else {
        return Err(CommandError::InternalServerError(
            "Failed to resolve image upload directory".to_string(),
        ));
    };
    fs::create_dir_all(parent).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to create image directory: {}", error))
    })?;

    let bytes = BASE64_STANDARD
        .decode(image_base64.as_bytes())
        .map_err(|error| CommandError::BadRequest(format!("Invalid image data: {}", error)))?;

    fs::write(&target_path, bytes).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to save the image: {}", error))
    })?;

    Ok(UserImageUploadResult {
        path: client_relative_path(&root_dir, &target_path)?,
    })
}

#[derive(Debug, Clone)]
struct ListedMediaFile {
    name: String,
    modified_ms: Option<i128>,
}

async fn list_media_files(
    directory: &Path,
    sort_field: &str,
    sort_order: &str,
    media_type: u32,
) -> Result<Vec<String>, CommandError> {
    let mut entries = fs::read_dir(directory).await.map_err(|error| {
        CommandError::InternalServerError(format!("Unable to read images directory: {}", error))
    })?;

    let sort_by_date = sort_field == "date";
    let sort_by_name = sort_field == "name";

    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        CommandError::InternalServerError(format!("Unable to read directory entry: {}", error))
    })? {
        let path = entry.path();

        let metadata = entry.metadata().await.map_err(|error| {
            CommandError::InternalServerError(format!("Unable to stat media file: {}", error))
        })?;
        if !metadata.is_file() {
            continue;
        }

        let Some(name) = path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
        else {
            continue;
        };

        let Some(mime) = mime_guess::from_path(&path).first() else {
            continue;
        };

        let is_match = ((media_type & MEDIA_REQUEST_IMAGE) != 0 && mime.type_() == "image")
            || ((media_type & MEDIA_REQUEST_VIDEO) != 0 && mime.type_() == "video")
            || ((media_type & MEDIA_REQUEST_AUDIO) != 0 && mime.type_() == "audio");
        if !is_match {
            continue;
        }

        let modified_ms = if sort_by_date {
            match metadata.modified() {
                Ok(value) => value
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|duration| duration.as_millis() as i128),
                Err(_) => None,
            }
        } else {
            None
        };

        files.push(ListedMediaFile { name, modified_ms });
    }

    if sort_by_name {
        files.sort_by(|left, right| left.name.cmp(&right.name));
    } else if sort_by_date {
        files.sort_by(|left, right| left.modified_ms.cmp(&right.modified_ms));
    }

    if sort_order == "desc" {
        files.reverse();
    }

    Ok(files.into_iter().map(|file| file.name).collect())
}

#[tauri::command]
pub async fn list_user_images(
    folder: String,
    sort_field: Option<String>,
    sort_order: Option<String>,
    media_type: Option<u32>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("list_user_images");

    let folder = folder.trim();
    if folder.is_empty() {
        return Err(CommandError::BadRequest("No folder specified".to_string()));
    }

    let sanitized_folder = sanitize_filename(folder);
    let sort_field = sort_field
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("date")
        .to_ascii_lowercase();
    let sort_order = sort_order
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("asc")
        .to_ascii_lowercase();
    let media_type = media_type.unwrap_or(MEDIA_REQUEST_IMAGE);

    let (_root_dir, images_dir) = get_default_user_image_directory(&app_state).await?;
    let target_dir = images_dir.join(sanitized_folder);

    fs::create_dir_all(&target_dir).await.map_err(|error| {
        CommandError::InternalServerError(format!("Unable to create directory: {}", error))
    })?;

    list_media_files(&target_dir, &sort_field, &sort_order, media_type).await
}

#[tauri::command]
pub async fn list_user_image_folders(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("list_user_image_folders");

    let (_root_dir, images_dir) = get_default_user_image_directory(&app_state).await?;
    let mut entries = fs::read_dir(&images_dir).await.map_err(|error| {
        CommandError::InternalServerError(format!("Unable to read images directory: {}", error))
    })?;

    let mut folders = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        CommandError::InternalServerError(format!("Unable to read directory entry: {}", error))
    })? {
        let metadata = entry.metadata().await.map_err(|error| {
            CommandError::InternalServerError(format!("Unable to stat folder: {}", error))
        })?;

        if !metadata.is_dir() {
            continue;
        }

        let Some(name) = entry.file_name().to_str().map(|value| value.to_string()) else {
            continue;
        };

        folders.push(name);
    }

    folders.sort();
    Ok(folders)
}

fn normalize_user_image_reference(raw: &str) -> Result<PathBuf, CommandError> {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return Err(CommandError::BadRequest("No path specified".to_string()));
    }

    if let Ok(parsed_url) = url::Url::parse(&value) {
        value = parsed_url.path().to_string();
    }

    let normalized = value.replace('\\', "/");
    let normalized = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{}", normalized)
    };

    let parsed = crate::infrastructure::user_data_paths::parse_user_data_asset_request_path(
        normalized.as_str(),
    )
    .map_err(|_| CommandError::BadRequest("Invalid path".to_string()))?
    .ok_or_else(|| CommandError::BadRequest("Invalid path".to_string()))?;

    if parsed.kind != crate::infrastructure::user_data_paths::UserDataAssetKind::UserImage {
        return Err(CommandError::BadRequest("Invalid path".to_string()));
    }

    Ok(parsed.relative_path)
}

#[tauri::command]
pub async fn delete_user_image(
    path: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("delete_user_image");

    let relative = normalize_user_image_reference(&path)?;
    let (_root_dir, images_dir) = get_default_user_image_directory(&app_state).await?;
    let target_path = images_dir.join(&relative);

    let metadata = fs::metadata(&target_path)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => CommandError::NotFound("File not found".to_string()),
            _ => CommandError::InternalServerError(format!("Failed to stat file: {}", error)),
        })?;

    if !metadata.is_file() {
        return Err(CommandError::NotFound("File not found".to_string()));
    }

    fs::remove_file(&target_path)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => CommandError::NotFound("File not found".to_string()),
            _ => CommandError::InternalServerError(format!("Failed to delete file: {}", error)),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{FileTime, set_file_mtime};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(test_name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("tauritavern-{test_name}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn sanitize_filename_replaces_illegal_chars() {
        assert_eq!(sanitize_filename("a:b*c?.png"), "a_b_c_.png");
    }

    #[test]
    fn remove_last_extension_strips_only_final_suffix() {
        assert_eq!(remove_last_extension("a.b.c"), "a.b");
        assert_eq!(remove_last_extension("file"), "file");
    }

    #[tokio::test]
    async fn list_media_files_filters_and_sorts_by_date() {
        let temp = TempDirGuard::new("images-list-date");
        let dir = temp.path.join("images");
        std::fs::create_dir_all(&dir).expect("create dir");

        let first = dir.join("a.png");
        let second = dir.join("b.mp4");
        let ignored = dir.join("c.txt");

        std::fs::write(&first, b"ok").expect("write file");
        std::fs::write(&second, b"ok").expect("write file");
        std::fs::write(&ignored, b"ok").expect("write file");

        set_file_mtime(&first, FileTime::from_unix_time(100, 0)).expect("mtime");
        set_file_mtime(&second, FileTime::from_unix_time(200, 0)).expect("mtime");

        let listed = list_media_files(
            &dir,
            "date",
            "asc",
            MEDIA_REQUEST_IMAGE | MEDIA_REQUEST_VIDEO,
        )
        .await
        .expect("list");

        assert_eq!(listed, vec!["a.png".to_string(), "b.mp4".to_string()]);
    }
}
