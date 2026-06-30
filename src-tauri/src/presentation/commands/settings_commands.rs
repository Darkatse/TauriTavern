use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::app::dev_observability::DevObservabilityHub;
use crate::application::dto::settings_dto::{
    SettingsSnapshotDto, SillyTavernSettingsResponseDto, TauriTavernSettingsDto,
    UpdateTauriTavernSettingsDto, UserSettingsDto,
};
use crate::application::services::host_resource_service::HostResourceService;
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_tauritavern_settings(
    app_state: State<'_, Arc<AppState>>,
) -> Result<TauriTavernSettingsDto, CommandError> {
    log_command("get_tauritavern_settings");

    app_state
        .services
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
    observability: State<'_, Arc<DevObservabilityHub>>,
    host_resources: State<'_, Arc<HostResourceService>>,
    tray_state: State<'_, Arc<crate::presentation::windows_tray::WindowsTrayState>>,
) -> Result<TauriTavernSettingsDto, CommandError> {
    log_command("update_tauritavern_settings");

    let agent_retention_settings_updated = has_agent_retention_settings_update(&dto);
    if dto
        .request_proxy
        .as_ref()
        .is_some_and(|settings| settings.enabled)
    {
        ensure_ios_policy_allows(
            &app_state.ios_policy,
            app_state.ios_policy.capabilities.network.request_proxy,
            "network.request_proxy",
        )?;
    }

    let settings = app_state
        .services
        .settings_service
        .update_tauritavern_settings(dto)
        .await
        .map_err(map_command_error("Failed to update TauriTavern settings"))?;

    tray_state.set_close_to_tray_on_close(settings.close_to_tray_on_close);
    host_resources.set_avatar_persona_original_images_enabled(
        settings.avatar_persona_original_images_enabled,
    );

    observability.apply_llm_api_log_retention(settings.dev.llm_api_keep);

    if agent_retention_settings_updated {
        app_state
            .services
            .agent_run_retention_automation_service
            .notify_settings_changed();
    }

    Ok(settings)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub async fn update_tauritavern_settings(
    dto: UpdateTauriTavernSettingsDto,
    app_state: State<'_, Arc<AppState>>,
    observability: State<'_, Arc<DevObservabilityHub>>,
    host_resources: State<'_, Arc<HostResourceService>>,
) -> Result<TauriTavernSettingsDto, CommandError> {
    log_command("update_tauritavern_settings");

    let agent_retention_settings_updated = has_agent_retention_settings_update(&dto);
    if dto
        .request_proxy
        .as_ref()
        .is_some_and(|settings| settings.enabled)
    {
        ensure_ios_policy_allows(
            &app_state.ios_policy,
            app_state.ios_policy.capabilities.network.request_proxy,
            "network.request_proxy",
        )?;
    }

    let settings = app_state
        .services
        .settings_service
        .update_tauritavern_settings(dto)
        .await
        .map_err(map_command_error("Failed to update TauriTavern settings"))?;

    host_resources.set_avatar_persona_original_images_enabled(
        settings.avatar_persona_original_images_enabled,
    );

    observability.apply_llm_api_log_retention(settings.dev.llm_api_keep);

    if agent_retention_settings_updated {
        app_state
            .services
            .agent_run_retention_automation_service
            .notify_settings_changed();
    }

    Ok(settings)
}

#[tauri::command]
pub async fn save_user_settings(
    settings: UserSettingsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_user_settings");

    app_state
        .services
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
        .services
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
        .services
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
        .services
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
        .services
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
        .services
        .settings_service
        .restore_snapshot(&name)
        .await
        .map_err(map_command_error("Failed to restore settings snapshot"))
}

fn has_agent_retention_settings_update(dto: &UpdateTauriTavernSettingsDto) -> bool {
    dto.agent
        .as_ref()
        .and_then(|agent| agent.retention.as_ref())
        .is_some()
}
