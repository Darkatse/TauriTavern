use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::services::asset_service::AssetDownloadResult;
use crate::domain::models::asset::AssetCatalog;
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_assets_library(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AssetCatalog, CommandError> {
    log_command("get_assets_library");

    app_state
        .services
        .asset_service
        .list_assets()
        .await
        .map_err(map_command_error("Failed to list assets library"))
}

#[tauri::command]
pub async fn download_asset(
    url: String,
    category: String,
    filename: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AssetDownloadResult, CommandError> {
    log_command(format!("download_asset {}", category));

    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.content.external_import,
        "content.external_import",
    )?;

    app_state
        .services
        .asset_service
        .download_asset(&url, &category, &filename)
        .await
        .map_err(map_command_error("Failed to download asset"))
}

#[tauri::command]
pub async fn delete_asset(
    category: String,
    filename: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_asset {}", category));

    app_state
        .services
        .asset_service
        .delete_asset_file(&category, &filename)
        .await
        .map_err(map_command_error("Failed to delete asset"))
}

#[tauri::command]
pub async fn get_character_assets(
    name: String,
    category: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!("get_character_assets {}", category));

    app_state
        .services
        .asset_service
        .list_character_assets(&name, &category)
        .await
        .map_err(map_command_error("Failed to list character assets"))
}
