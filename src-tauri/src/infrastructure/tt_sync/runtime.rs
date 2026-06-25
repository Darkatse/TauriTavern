use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use ttsync_contract::peer::DeviceId;

use crate::domain::errors::DomainError;
use crate::domain::models::sync::SyncOrigin;
use crate::domain::models::tt_sync::{TtSyncPairedServer, TtSyncProgressEvent};
use crate::infrastructure::tt_sync::store::TtSyncStore;

pub struct TtSyncRuntime {
    app_handle: AppHandle,
    pub sync_root: PathBuf,
    pub store: TtSyncStore,
    paired_servers_cache: Mutex<Option<HashMap<String, TtSyncPairedServer>>>,
}

impl TtSyncRuntime {
    pub fn new(app_handle: AppHandle, sync_root: PathBuf, store_root: PathBuf) -> Self {
        Self {
            app_handle,
            sync_root,
            store: TtSyncStore::new(store_root),
            paired_servers_cache: Mutex::new(None),
        }
    }

    pub async fn load_paired_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError> {
        let cached = {
            let cache = self.paired_servers_cache.lock().await;
            cache
                .as_ref()
                .map(|servers| servers.values().cloned().collect::<Vec<_>>())
        };
        if let Some(servers) = cached {
            return Ok(servers);
        }

        let servers = self.store.load_paired_servers().await?;
        let map = servers
            .iter()
            .cloned()
            .map(|server| (server.server_device_id.to_string(), server))
            .collect::<HashMap<_, _>>();
        {
            let mut cache = self.paired_servers_cache.lock().await;
            if cache.is_none() {
                *cache = Some(map);
            }
        }

        Ok(servers)
    }

    pub async fn get_paired_server(
        &self,
        server_device_id: &DeviceId,
    ) -> Result<TtSyncPairedServer, DomainError> {
        let cached = {
            let cache = self.paired_servers_cache.lock().await;
            cache
                .as_ref()
                .and_then(|map| map.get(server_device_id.as_str()).cloned())
        };
        if let Some(server) = cached {
            return Ok(server);
        }

        let servers = self.store.load_paired_servers().await?;
        let map = servers
            .into_iter()
            .map(|server| (server.server_device_id.to_string(), server))
            .collect::<HashMap<_, _>>();

        let result = map.get(server_device_id.as_str()).cloned().ok_or_else(|| {
            DomainError::NotFound(format!(
                "Paired TT-Sync server not found: {}",
                server_device_id
            ))
        })?;

        {
            let mut cache = self.paired_servers_cache.lock().await;
            if cache.is_none() {
                *cache = Some(map);
            }
        }

        Ok(result)
    }

    pub async fn upsert_paired_server(
        &self,
        server: TtSyncPairedServer,
    ) -> Result<(), DomainError> {
        self.store.upsert_paired_server(server.clone()).await?;

        let mut cache = self.paired_servers_cache.lock().await;
        if let Some(map) = cache.as_mut() {
            map.insert(server.server_device_id.to_string(), server);
        }

        Ok(())
    }

    pub async fn remove_paired_server(
        &self,
        server_device_id: &DeviceId,
    ) -> Result<(), DomainError> {
        self.store.remove_paired_server(server_device_id).await?;

        let mut cache = self.paired_servers_cache.lock().await;
        if let Some(map) = cache.as_mut() {
            map.remove(server_device_id.as_str());
        }

        Ok(())
    }

    pub fn emit_progress(&self, payload: TtSyncProgressEvent, origin: &SyncOrigin) {
        self.emit_with_origin("tt_sync:progress", payload, origin)
    }

    fn emit_with_origin<T: Serialize>(&self, event: &str, payload: T, origin: &SyncOrigin) {
        let mut payload = match serde_json::to_value(payload) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!("Failed to serialize TT-Sync event payload: {}", error);
                return;
            }
        };
        if let serde_json::Value::Object(map) = &mut payload {
            map.insert(
                "origin".to_string(),
                serde_json::Value::String(event_origin(origin).to_string()),
            );
        }

        if let Err(error) = self.app_handle.emit(event, payload) {
            tracing::warn!("Failed to emit TT-Sync event '{event}': {}", error);
        }
    }
}

fn event_origin(origin: &SyncOrigin) -> &'static str {
    match origin {
        SyncOrigin::Scheduled => "auto",
        SyncOrigin::Manual | SyncOrigin::RemoteRequest { .. } => "manual",
    }
}
