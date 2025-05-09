use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::settings_dto::{
    AppSettingsDto, UpdateAppSettingsDto, UserSettingsDto,
    SettingsSnapshotDto, SillyTavernSettingsResponseDto
};
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

#[tauri::command]
pub async fn get_settings(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AppSettingsDto, CommandError> {
    logger::debug("Command: get_settings");

    app_state.settings_service.get_settings().await
        .map_err(|e| {
            logger::error(&format!("Failed to get settings: {}", e));
            e.into()
        })
}

#[tauri::command]
pub async fn update_settings(
    dto: UpdateAppSettingsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AppSettingsDto, CommandError> {
    logger::debug("Command: update_settings");

    app_state.settings_service.update_settings(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to update settings: {}", e));
            e.into()
        })
}

// SillyTavern 设置 API

/// 保存用户设置
#[tauri::command]
pub async fn save_user_settings(
    settings: UserSettingsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug("Command: save_user_settings");

    app_state.settings_service.save_user_settings(settings).await
        .map_err(|e| {
            logger::error(&format!("Failed to save user settings: {}", e));
            e.into()
        })
}

/// 获取 SillyTavern 设置
#[tauri::command]
pub async fn get_sillytavern_settings(
    app_state: State<'_, Arc<AppState>>,
) -> Result<SillyTavernSettingsResponseDto, CommandError> {
    logger::debug("Command: get_sillytavern_settings");

    app_state.settings_service.get_sillytavern_settings().await
        .map_err(|e| {
            logger::error(&format!("Failed to get SillyTavern settings: {}", e));
            e.into()
        })
}

/// 创建设置快照
#[tauri::command]
pub async fn create_settings_snapshot(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug("Command: create_settings_snapshot");

    app_state.settings_service.create_snapshot().await
        .map_err(|e| {
            logger::error(&format!("Failed to create settings snapshot: {}", e));
            e.into()
        })
}

/// 获取设置快照列表
#[tauri::command]
pub async fn get_settings_snapshots(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SettingsSnapshotDto>, CommandError> {
    logger::debug("Command: get_settings_snapshots");

    app_state.settings_service.get_snapshots().await
        .map_err(|e| {
            logger::error(&format!("Failed to get settings snapshots: {}", e));
            e.into()
        })
}

/// 加载设置快照
#[tauri::command]
pub async fn load_settings_snapshot(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserSettingsDto, CommandError> {
    logger::debug(&format!("Command: load_settings_snapshot - {}", name));

    app_state.settings_service.load_snapshot(&name).await
        .map_err(|e| {
            logger::error(&format!("Failed to load settings snapshot: {}", e));
            e.into()
        })
}

/// 恢复设置快照
#[tauri::command]
pub async fn restore_settings_snapshot(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: restore_settings_snapshot - {}", name));

    app_state.settings_service.restore_snapshot(&name).await
        .map_err(|e| {
            logger::error(&format!("Failed to restore settings snapshot: {}", e));
            e.into()
        })
}
