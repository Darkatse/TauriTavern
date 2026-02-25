use tauri::{command, State};
use crate::infrastructure::sync::lan_server::{LanSyncServer, SyncServerStatus};

#[command]
pub async fn plugin_lan_sync_start(server: State<'_, LanSyncServer>) -> Result<String, String> {
    server.start().await
}

#[command]
pub async fn plugin_lan_sync_stop(server: State<'_, LanSyncServer>) -> Result<(), String> {
    server.stop().await;
    Ok(())
}

#[command]
pub async fn plugin_lan_sync_status(server: State<'_, LanSyncServer>) -> Result<SyncServerStatus, String> {
    Ok(server.get_status().await)
}

#[command]
pub async fn plugin_lan_sync_qr(server: State<'_, LanSyncServer>, text: String) -> Result<String, String> {
    server.generate_qr_code(&text)
}
