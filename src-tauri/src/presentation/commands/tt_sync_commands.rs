use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use ttsync_contract::sync::SyncMode;

use crate::app::AppState;
use crate::domain::models::tt_sync::TtSyncPairedServer;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[derive(Debug, Clone, Serialize)]
pub struct TtSyncPermissionsDto {
    pub read: bool,
    pub write: bool,
    pub mirror_delete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TtSyncPairedServerDto {
    pub server_device_id: String,
    pub server_device_name: String,
    pub base_url: String,
    pub spki_sha256: String,
    pub permissions: TtSyncPermissionsDto,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}

impl From<TtSyncPairedServer> for TtSyncPairedServerDto {
    fn from(server: TtSyncPairedServer) -> Self {
        Self {
            server_device_id: server.server_device_id.to_string(),
            server_device_name: server.server_device_name,
            base_url: server.base_url,
            spki_sha256: server.spki_sha256,
            permissions: TtSyncPermissionsDto {
                read: server.permissions.read,
                write: server.permissions.write,
                mirror_delete: server.permissions.mirror_delete,
            },
            paired_at_ms: server.paired_at_ms,
            last_sync_ms: server.last_sync_ms,
        }
    }
}

#[tauri::command]
pub async fn tt_sync_pair(
    app_state: State<'_, Arc<AppState>>,
    pair_uri: String,
) -> Result<TtSyncPairedServerDto, CommandError> {
    log_command("tt_sync_pair");

    app_state
        .tt_sync_service
        .pair(&pair_uri)
        .await
        .map(TtSyncPairedServerDto::from)
        .map_err(map_command_error("Failed to pair TT-Sync server"))
}

#[tauri::command]
pub async fn tt_sync_list_servers(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<TtSyncPairedServerDto>, CommandError> {
    log_command("tt_sync_list_servers");

    app_state
        .tt_sync_service
        .list_servers()
        .await
        .map(|servers| {
            servers
                .into_iter()
                .map(TtSyncPairedServerDto::from)
                .collect()
        })
        .map_err(map_command_error("Failed to list TT-Sync servers"))
}

#[tauri::command]
pub async fn tt_sync_remove_server(
    app_state: State<'_, Arc<AppState>>,
    server_device_id: String,
) -> Result<(), CommandError> {
    log_command("tt_sync_remove_server");

    app_state
        .tt_sync_service
        .remove_server(&server_device_id)
        .await
        .map_err(map_command_error("Failed to remove TT-Sync server"))
}

#[tauri::command]
pub async fn tt_sync_pull(
    app_state: State<'_, Arc<AppState>>,
    server_device_id: String,
    mode: SyncMode,
) -> Result<(), CommandError> {
    log_command("tt_sync_pull");

    app_state
        .tt_sync_service
        .pull(&server_device_id, mode)
        .await
        .map_err(map_command_error("Failed to run TT-Sync pull"))
}

#[tauri::command]
pub async fn tt_sync_push(
    app_state: State<'_, Arc<AppState>>,
    server_device_id: String,
    mode: SyncMode,
) -> Result<(), CommandError> {
    log_command("tt_sync_push");

    app_state
        .tt_sync_service
        .push(&server_device_id, mode)
        .await
        .map_err(map_command_error("Failed to run TT-Sync push"))
}
