use std::sync::Arc;
use tauri::State;
use crate::app::AppState;
use crate::application::dto::background_dto::{BackgroundDto, DeleteBackgroundDto, RenameBackgroundDto};
use crate::application::errors::ApplicationError;
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

/// Get all background images
///
/// Returns a simple array of filenames to maintain compatibility with SillyTavern
#[tauri::command]
pub async fn get_all_backgrounds(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    logger::debug("Command: get_all_backgrounds");

    app_state.background_service.get_all_backgrounds().await
        .map(|backgrounds| backgrounds.into_iter().map(|bg| bg.filename).collect())
        .map_err(|e| {
            logger::error(&format!("Failed to get all backgrounds: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Delete a background image
#[tauri::command]
pub async fn delete_background(
    app_state: State<'_, Arc<AppState>>,
    dto: DeleteBackgroundDto,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_background, filename: {}", dto.bg));

    app_state.background_service.delete_background(&dto.bg).await
        .map_err(|e| {
            logger::error(&format!("Failed to delete background: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Rename a background image
#[tauri::command]
pub async fn rename_background(
    app_state: State<'_, Arc<AppState>>,
    dto: RenameBackgroundDto,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: rename_background, from: {} to: {}", dto.old_bg, dto.new_bg));

    app_state.background_service.rename_background(&dto.old_bg, &dto.new_bg).await
        .map_err(|e| {
            logger::error(&format!("Failed to rename background: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Upload a background image
#[tauri::command]
pub async fn upload_background(
    app_state: State<'_, Arc<AppState>>,
    filename: String,
    data: Vec<u8>,
) -> Result<String, CommandError> {
    logger::debug(&format!("Command: upload_background, filename: {}", filename));

    app_state.background_service.upload_background(&filename, &data).await
        .map_err(|e| {
            logger::error(&format!("Failed to upload background: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}
