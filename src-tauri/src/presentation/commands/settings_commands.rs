use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::settings_dto::{
    AppSettingsDto, SettingsSnapshotDto, SillyTavernSettingsResponseDto, UpdateAppSettingsDto,
    UserSettingsDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_settings(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AppSettingsDto, CommandError> {
    log_command("get_settings");

    app_state
        .settings_service
        .get_settings()
        .await
        .map_err(map_command_error("Failed to get settings"))
}

#[tauri::command]
pub async fn update_settings(
    dto: UpdateAppSettingsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AppSettingsDto, CommandError> {
    log_command("update_settings");

    app_state
        .settings_service
        .update_settings(dto)
        .await
        .map_err(map_command_error("Failed to update settings"))
}

#[tauri::command]
pub async fn save_user_settings(
    settings: UserSettingsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_user_settings");

    app_state
        .settings_service
        .save_user_settings(settings)
        .await
        .map_err(map_command_error("Failed to save user settings"))
}

#[tauri::command]
pub async fn get_sillytavern_settings(
    app_state: State<'_, Arc<AppState>>,
) -> Result<SillyTavernSettingsResponseDto, CommandError> {
    log_command("get_sillytavern_settings");

    app_state
        .settings_service
        .get_sillytavern_settings()
        .await
        .map_err(map_command_error("Failed to get SillyTavern settings"))
}

#[tauri::command]
pub async fn create_settings_snapshot(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("create_settings_snapshot");

    app_state
        .settings_service
        .create_snapshot()
        .await
        .map_err(map_command_error("Failed to create settings snapshot"))
}

#[tauri::command]
pub async fn get_settings_snapshots(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SettingsSnapshotDto>, CommandError> {
    log_command("get_settings_snapshots");

    app_state
        .settings_service
        .get_snapshots()
        .await
        .map_err(map_command_error("Failed to get settings snapshots"))
}

#[tauri::command]
pub async fn load_settings_snapshot(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserSettingsDto, CommandError> {
    log_command(format!("load_settings_snapshot - {}", name));

    app_state
        .settings_service
        .load_snapshot(&name)
        .await
        .map_err(map_command_error("Failed to load settings snapshot"))
}

#[tauri::command]
pub async fn restore_settings_snapshot(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("restore_settings_snapshot - {}", name));

    app_state
        .settings_service
        .restore_snapshot(&name)
        .await
        .map_err(map_command_error("Failed to restore settings snapshot"))
}
