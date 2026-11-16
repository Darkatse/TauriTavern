use std::sync::Arc;

use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::app::dev_observability::DevObservabilityHub;
use crate::presentation::commands::chunk_body::{chunk_bytes_from_request, commit_headers};
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::commands::keys_exposure::ensure_keys_exposure_consent;
use crate::presentation::commands::request_proxy::ensure_request_proxy_consent;
use crate::presentation::errors::CommandError;
use tt_application::dto::settings_dto::{
    RequestProxySettingsDto, SettingsSnapshotDto, TauriTavernSettingsDto,
    UpdateTauriTavernSettingsDto, UserSettingsDto, UserSettingsRevisionDto,
    UserSettingsSaveResultDto,
};
use tt_application::services::host_resource_service::HostResourceService;
use tt_contracts::byte_commit::CommitBegin;
use tt_contracts::chat::ChatBackupStorageStats;
use tt_domain::models::settings::RequestProxySettings;

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

#[tauri::command]
pub async fn get_chat_backup_storage_stats(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<ChatBackupStorageStats>, CommandError> {
    log_command("get_chat_backup_storage_stats");

    app_state
        .services
        .settings_service
        .get_chat_backup_storage_stats()
        .await
        .map_err(map_command_error("Failed to get chat backup storage stats"))
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
pub async fn set_keys_exposure(
    allowed: bool,
    locale: String,
    app_state: State<'_, Arc<AppState>>,
    window: WebviewWindow,
) -> Result<(), CommandError> {
    log_command(format!("set_keys_exposure {allowed}"));

    if allowed {
        ensure_keys_exposure_consent(&window, &locale).await?;
    }
    app_state
        .services
        .settings_service
        .set_keys_exposure(allowed)
        .await
        .map_err(map_command_error("Failed to set keys exposure"))
}

#[tauri::command]
pub async fn set_request_proxy(
    settings: RequestProxySettingsDto,
    locale: String,
    app_state: State<'_, Arc<AppState>>,
    window: WebviewWindow,
) -> Result<(), CommandError> {
    log_command("set_request_proxy");

    let proxy = RequestProxySettings::from(settings);
    if proxy.enabled {
        ensure_ios_policy_allows(
            &app_state.ios_policy,
            app_state.ios_policy.capabilities.network.request_proxy,
            "network.request_proxy",
        )?;
    }
    if let Some(server) = proxy
        .proxy_url()
        .map_err(map_command_error("Failed to set request proxy"))?
    {
        ensure_request_proxy_consent(&window, &locale, &server).await?;
    }
    app_state
        .services
        .settings_service
        .set_request_proxy(proxy)
        .await
        .map_err(map_command_error("Failed to set request proxy"))
}

#[tauri::command]
pub async fn begin_settings_commit(
    expected_revision: Option<UserSettingsRevisionDto>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CommitBegin, CommandError> {
    app_state
        .services
        .settings_service
        .begin_commit(expected_revision)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn append_settings_commit_chunk(
    request: tauri::ipc::Request<'_>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<u64, CommandError> {
    let (session_id, offset) = commit_headers(&request)?;
    let bytes = chunk_bytes_from_request(&request)?;
    app_state
        .services
        .settings_service
        .append_commit(session_id, offset, &bytes)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn finish_settings_commit(
    session_id: String,
    expected_size: u64,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserSettingsSaveResultDto, CommandError> {
    app_state
        .services
        .settings_service
        .finish_commit(&session_id, expected_size)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn abort_settings_commit(
    session_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .settings_service
        .abort_commit(&session_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_sillytavern_settings(
    app_state: State<'_, Arc<AppState>>,
) -> Result<tauri::ipc::Response, CommandError> {
    let result = app_state
        .services
        .settings_service
        .get_sillytavern_settings()
        .await
        .map_err(map_command_error("Failed to get SillyTavern settings"))?;
    Ok(tauri::ipc::Response::new(result.bytes.to_vec()))
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
