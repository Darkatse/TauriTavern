use tokio::sync::Mutex;
use ttsync_contract::sync::SyncMode;

#[derive(Default)]
pub struct LanSyncRuntimeState {
    sync_mode_override: Mutex<Option<SyncMode>>,
}

impl LanSyncRuntimeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_sync_mode_override(&self) -> Option<SyncMode> {
        *self.sync_mode_override.lock().await
    }

    pub async fn set_sync_mode_override(&self, mode: Option<SyncMode>) {
        *self.sync_mode_override.lock().await = mode;
    }
}
