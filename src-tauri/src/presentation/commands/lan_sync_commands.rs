use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::app::AppState;
use crate::domain::models::lan_sync::{LanSyncPairedDevice, LanSyncStatus, LanSyncSyncMode};
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::errors::CommandError;

fn ensure_lan_sync_allowed(app_state: &AppState) -> Result<(), CommandError> {
    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.sync.lan,
        "sync.lan",
    )
}

#[tauri::command]
pub async fn lan_sync_get_status(
    app_state: State<'_, Arc<AppState>>,
) -> Result<LanSyncStatus, CommandError> {
    log_command("lan_sync_get_status");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .get_status()
        .await
        .map_err(map_command_error("Failed to get LAN sync status"))
}

#[tauri::command]
pub async fn lan_sync_start_server(
    app_state: State<'_, Arc<AppState>>,
) -> Result<LanSyncStatus, CommandError> {
    log_command("lan_sync_start_server");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .start_server()
        .await
        .map_err(map_command_error("Failed to start LAN sync server"))
}

#[tauri::command]
pub async fn lan_sync_stop_server(app_state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    log_command("lan_sync_stop_server");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .stop_server()
        .await
        .map_err(map_command_error("Failed to stop LAN sync server"))
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairingInfoDto {
    pub address: String,
    pub pair_uri: String,
    pub qr_svg: String,
    pub expires_at_ms: u64,
}

#[tauri::command]
pub async fn lan_sync_enable_pairing(
    app_state: State<'_, Arc<AppState>>,
    address: Option<String>,
) -> Result<LanSyncPairingInfoDto, CommandError> {
    log_command("lan_sync_enable_pairing");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .enable_pairing(address)
        .await
        .map(|info| LanSyncPairingInfoDto {
            address: info.address,
            pair_uri: info.pair_uri,
            qr_svg: info.qr_svg,
            expires_at_ms: info.expires_at_ms,
        })
        .map_err(map_command_error("Failed to enable LAN sync pairing"))
}

#[tauri::command]
pub async fn lan_sync_get_pairing_info(
    app_state: State<'_, Arc<AppState>>,
    address: String,
) -> Result<LanSyncPairingInfoDto, CommandError> {
    log_command("lan_sync_get_pairing_info");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .get_pairing_info(&address)
        .await
        .map(|info| LanSyncPairingInfoDto {
            address: info.address,
            pair_uri: info.pair_uri,
            qr_svg: info.qr_svg,
            expires_at_ms: info.expires_at_ms,
        })
        .map_err(map_command_error("Failed to get LAN sync pairing info"))
}

#[derive(Debug, Clone, Serialize)]
pub struct LanSyncPairedDeviceDto {
    pub device_id: String,
    pub device_name: String,
    pub last_known_address: Option<String>,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}

impl From<LanSyncPairedDevice> for LanSyncPairedDeviceDto {
    fn from(device: LanSyncPairedDevice) -> Self {
        Self {
            device_id: device.device_id,
            device_name: device.device_name,
            last_known_address: device.last_known_address,
            paired_at_ms: device.paired_at_ms,
            last_sync_ms: device.last_sync_ms,
        }
    }
}

#[tauri::command]
pub async fn lan_sync_request_pairing(
    app_state: State<'_, Arc<AppState>>,
    pair_uri: String,
) -> Result<LanSyncPairedDeviceDto, CommandError> {
    log_command("lan_sync_request_pairing");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .request_pairing(&pair_uri)
        .await
        .map(LanSyncPairedDeviceDto::from)
        .map_err(map_command_error("Failed to request LAN sync pairing"))
}

#[tauri::command]
pub async fn lan_sync_confirm_pairing(
    app_state: State<'_, Arc<AppState>>,
    request_id: String,
    accept: bool,
) -> Result<(), CommandError> {
    log_command("lan_sync_confirm_pairing");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .confirm_pairing(&request_id, accept)
        .await
        .map_err(map_command_error("Failed to confirm LAN sync pairing"))
}

#[tauri::command]
pub async fn lan_sync_list_devices(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<LanSyncPairedDeviceDto>, CommandError> {
    log_command("lan_sync_list_devices");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .list_paired_devices()
        .await
        .map(|devices| {
            devices
                .into_iter()
                .map(LanSyncPairedDeviceDto::from)
                .collect()
        })
        .map_err(map_command_error("Failed to list LAN sync devices"))
}

#[tauri::command]
pub async fn lan_sync_remove_device(
    app_state: State<'_, Arc<AppState>>,
    device_id: String,
) -> Result<(), CommandError> {
    log_command("lan_sync_remove_device");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .remove_paired_device(&device_id)
        .await
        .map_err(map_command_error("Failed to remove LAN sync device"))
}

#[tauri::command]
pub async fn lan_sync_sync_from_device(
    app_state: State<'_, Arc<AppState>>,
    device_id: String,
) -> Result<(), CommandError> {
    log_command("lan_sync_sync_from_device");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .sync_from_device(&device_id)
        .await
        .map_err(map_command_error("Failed to run LAN sync pull"))
}

#[tauri::command]
pub async fn lan_sync_push_to_device(
    app_state: State<'_, Arc<AppState>>,
    device_id: String,
) -> Result<(), CommandError> {
    log_command("lan_sync_push_to_device");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .push_to_device(&device_id)
        .await
        .map_err(map_command_error("Failed to request LAN sync push"))
}

#[tauri::command]
pub async fn lan_sync_set_sync_mode(
    app_state: State<'_, Arc<AppState>>,
    mode: LanSyncSyncMode,
    persist: bool,
) -> Result<(), CommandError> {
    log_command("lan_sync_set_sync_mode");
    ensure_lan_sync_allowed(&app_state)?;

    app_state
        .lan_sync_service
        .set_sync_mode(mode, persist)
        .await
        .map_err(map_command_error("Failed to set LAN sync mode"))
}

#[tauri::command]
pub async fn lan_sync_clear_sync_mode_override(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("lan_sync_clear_sync_mode_override");
    ensure_lan_sync_allowed(&app_state)?;

    app_state.lan_sync_service.clear_sync_mode_override().await;
    Ok(())
}
