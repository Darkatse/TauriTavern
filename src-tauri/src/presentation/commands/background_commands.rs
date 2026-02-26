use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::background_dto::{DeleteBackgroundDto, RenameBackgroundDto};
use crate::domain::models::background::BackgroundImageMetadataIndex;
use crate::infrastructure::persistence::thumbnail_cache::{
    ThumbnailConfig, ThumbnailResizeMode, read_thumbnail_or_original,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

const AVATAR_THUMBNAIL_WIDTH: u32 = 96;
const AVATAR_THUMBNAIL_HEIGHT: u32 = 144;
const AVATAR_THUMBNAIL_QUALITY: u8 = 90;

#[derive(Debug, Clone, Serialize)]
pub struct ThumbnailAssetPayload {
    pub content_base64: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Copy)]
enum ThumbnailType {
    Bg,
    Avatar,
    Persona,
}

impl ThumbnailType {
    fn parse(value: &str) -> Result<Self, CommandError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "bg" => Ok(Self::Bg),
            "avatar" => Ok(Self::Avatar),
            "persona" => Ok(Self::Persona),
            _ => Err(CommandError::BadRequest(
                "Invalid thumbnail type".to_string(),
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bg => "bg",
            Self::Avatar => "avatar",
            Self::Persona => "persona",
        }
    }
}

fn sanitize_thumbnail_filename(filename: &str) -> Result<String, CommandError> {
    let sanitized = filename
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ if character.is_control() => '_',
            _ => character,
        })
        .collect::<String>();
    let sanitized = sanitized.trim().trim_end_matches(['.', ' ']).to_string();

    if sanitized.is_empty() {
        return Err(CommandError::BadRequest(
            "Invalid thumbnail file name".to_string(),
        ));
    }

    Ok(sanitized)
}

async fn read_non_background_thumbnail_asset(
    app_state: &Arc<AppState>,
    thumbnail_type: ThumbnailType,
    file: &str,
) -> Result<ThumbnailAssetPayload, CommandError> {
    let directories = app_state
        .user_directory_service
        .get_default_user_directory()
        .await
        .map_err(map_command_error(
            "Failed to resolve default user directories for thumbnail",
        ))?;
    let safe_file_name = sanitize_thumbnail_filename(file)?;

    let (source_directory, thumbnail_directory) = match thumbnail_type {
        ThumbnailType::Avatar => (directories.characters, directories.thumbnails_avatar),
        ThumbnailType::Persona => (directories.avatars, directories.thumbnails_persona),
        ThumbnailType::Bg => {
            return Err(CommandError::BadRequest(
                "Unsupported non-background thumbnail type".to_string(),
            ));
        }
    };

    let original_path = std::path::PathBuf::from(source_directory).join(&safe_file_name);
    let thumbnail_path = std::path::PathBuf::from(thumbnail_directory).join(&safe_file_name);
    let asset = read_thumbnail_or_original(
        &original_path,
        &thumbnail_path,
        ThumbnailConfig {
            width: AVATAR_THUMBNAIL_WIDTH,
            height: AVATAR_THUMBNAIL_HEIGHT,
            quality: AVATAR_THUMBNAIL_QUALITY,
            resize_mode: ThumbnailResizeMode::Cover,
        },
    )
    .await
    .map_err(map_command_error("Failed to read non-background thumbnail"))?;

    Ok(ThumbnailAssetPayload {
        content_base64: BASE64_STANDARD.encode(asset.bytes),
        mime_type: asset.mime_type,
    })
}

#[tauri::command]
pub async fn get_all_backgrounds(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("get_all_backgrounds");

    app_state
        .background_service
        .get_all_backgrounds()
        .await
        .map(|backgrounds| backgrounds.into_iter().map(|bg| bg.filename).collect())
        .map_err(map_command_error("Failed to get all backgrounds"))
}

#[tauri::command]
pub async fn get_all_background_metadata(
    app_state: State<'_, Arc<AppState>>,
    prefix: Option<String>,
) -> Result<BackgroundImageMetadataIndex, CommandError> {
    log_command(format!(
        "get_all_background_metadata, prefix: {}",
        prefix.clone().unwrap_or_default()
    ));

    app_state
        .background_service
        .get_all_background_metadata(prefix.as_deref())
        .await
        .map_err(map_command_error("Failed to get background metadata"))
}

#[tauri::command]
pub async fn delete_background(
    app_state: State<'_, Arc<AppState>>,
    dto: DeleteBackgroundDto,
) -> Result<(), CommandError> {
    log_command(format!("delete_background, filename: {}", dto.bg));

    app_state
        .background_service
        .delete_background(&dto.bg)
        .await
        .map_err(map_command_error("Failed to delete background"))
}

#[tauri::command]
pub async fn rename_background(
    app_state: State<'_, Arc<AppState>>,
    dto: RenameBackgroundDto,
) -> Result<(), CommandError> {
    log_command(format!(
        "rename_background, from: {} to: {}",
        dto.old_bg, dto.new_bg
    ));

    app_state
        .background_service
        .rename_background(&dto.old_bg, &dto.new_bg)
        .await
        .map_err(map_command_error("Failed to rename background"))
}

#[tauri::command]
pub async fn upload_background(
    app_state: State<'_, Arc<AppState>>,
    filename: String,
    data: Vec<u8>,
) -> Result<String, CommandError> {
    log_command(format!("upload_background, filename: {}", filename));

    app_state
        .background_service
        .upload_background(&filename, &data)
        .await
        .map_err(map_command_error("Failed to upload background"))
}

#[tauri::command]
pub async fn read_thumbnail_asset(
    app_state: State<'_, Arc<AppState>>,
    thumbnail_type: String,
    file: String,
    animated: Option<bool>,
) -> Result<ThumbnailAssetPayload, CommandError> {
    let thumbnail_type = ThumbnailType::parse(&thumbnail_type)?;
    log_command(format!(
        "read_thumbnail_asset type={} file={}",
        thumbnail_type.as_str(),
        file
    ));

    match thumbnail_type {
        ThumbnailType::Bg => {
            let asset = app_state
                .background_service
                .read_background_thumbnail(&file, animated.unwrap_or(false))
                .await
                .map_err(map_command_error("Failed to read background thumbnail"))?;

            Ok(ThumbnailAssetPayload {
                content_base64: BASE64_STANDARD.encode(asset.bytes),
                mime_type: asset.mime_type,
            })
        }
        ThumbnailType::Avatar | ThumbnailType::Persona => {
            read_non_background_thumbnail_asset(app_state.inner(), thumbnail_type, &file).await
        }
    }
}
