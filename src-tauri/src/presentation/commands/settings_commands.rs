use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::settings_dto::{
    SettingsSnapshotDto, SillyTavernSettingsResponseDto, TauriTavernSettingsDto,
    UpdateTauriTavernSettingsDto, UserSettingsDto,
};
use crate::domain::models::settings::RequestProxySettings;
use crate::infrastructure::http_client_pool::HttpClientPool;
use crate::infrastructure::logging::llm_api_logs::LlmApiLogStore;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use crate::presentation::web_resources::thumbnail_endpoint::ThumbnailEndpointPolicy;

#[tauri::command]
pub async fn get_tauritavern_settings(
    app_state: State<'_, Arc<AppState>>,
) -> Result<TauriTavernSettingsDto, CommandError> {
    log_command("get_tauritavern_settings");

    app_state
        .settings_service
        .get_tauritavern_settings()
        .await
        .map_err(map_command_error("Failed to get TauriTavern settings"))
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn update_tauritavern_settings(
    dto: UpdateTauriTavernSettingsDto,
    app_state: State<'_, Arc<AppState>>,
    http_clients: State<'_, Arc<HttpClientPool>>,
    llm_api_logs: State<'_, Arc<LlmApiLogStore>>,
    thumbnail_policy: State<'_, Arc<ThumbnailEndpointPolicy>>,
    tray_state: State<'_, Arc<crate::presentation::windows_tray::WindowsTrayState>>,
) -> Result<TauriTavernSettingsDto, CommandError> {
    log_command("update_tauritavern_settings");

    let request_proxy_settings: Option<RequestProxySettings> =
        dto.request_proxy.clone().map(Into::into);
    if let Some(settings) = request_proxy_settings.as_ref() {
        HttpClientPool::validate_request_proxy_settings(settings)
            .map_err(map_command_error("Invalid request proxy settings"))?;
    }

    let settings = app_state
        .settings_service
        .update_tauritavern_settings(dto)
        .await
        .map_err(map_command_error("Failed to update TauriTavern settings"))?;

    tray_state.set_close_to_tray_on_close(settings.close_to_tray_on_close);
    thumbnail_policy.set_avatar_persona_original_images_enabled(
        settings.avatar_persona_original_images_enabled,
    );

    if request_proxy_settings.is_some() {
        http_clients
            .apply_request_proxy_settings(&settings.request_proxy.clone().into())
            .map_err(map_command_error("Failed to apply request proxy settings"))?;
    }

    llm_api_logs.apply_settings(settings.dev.llm_api_keep);

    Ok(settings)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub async fn update_tauritavern_settings(
    dto: UpdateTauriTavernSettingsDto,
    app_state: State<'_, Arc<AppState>>,
    http_clients: State<'_, Arc<HttpClientPool>>,
    llm_api_logs: State<'_, Arc<LlmApiLogStore>>,
    thumbnail_policy: State<'_, Arc<ThumbnailEndpointPolicy>>,
) -> Result<TauriTavernSettingsDto, CommandError> {
    log_command("update_tauritavern_settings");

    let request_proxy_settings: Option<RequestProxySettings> =
        dto.request_proxy.clone().map(Into::into);
    if let Some(settings) = request_proxy_settings.as_ref() {
        HttpClientPool::validate_request_proxy_settings(settings)
            .map_err(map_command_error("Invalid request proxy settings"))?;
    }

    let settings = app_state
        .settings_service
        .update_tauritavern_settings(dto)
        .await
        .map_err(map_command_error("Failed to update TauriTavern settings"))?;

    thumbnail_policy.set_avatar_persona_original_images_enabled(
        settings.avatar_persona_original_images_enabled,
    );

    if request_proxy_settings.is_some() {
        http_clients
            .apply_request_proxy_settings(&settings.request_proxy.clone().into())
            .map_err(map_command_error("Failed to apply request proxy settings"))?;
    }

    llm_api_logs.apply_settings(settings.dev.llm_api_keep);

    Ok(settings)
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
