use std::collections::HashMap;
use std::path::PathBuf;

use tokio::sync::Mutex;

use ttsync_contract::peer::DeviceId;

use crate::tt_sync::store::TtSyncStore;
use tt_domain::errors::DomainError;
use tt_domain::models::tt_sync::{TtSyncIdentity, TtSyncPairedServer};
use tt_ports::sync::TtSyncRepository;

pub struct TtSyncRuntime {
    pub sync_root: PathBuf,
    pub store: TtSyncStore,
    paired_servers_cache: Mutex<Option<HashMap<String, TtSyncPairedServer>>>,
}

#[async_trait::async_trait]
impl TtSyncRepository for TtSyncRuntime {
    async fn load_or_create_identity(&self) -> Result<TtSyncIdentity, DomainError> {
        self.store.load_or_create_identity().await
    }

    async fn load_paired_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError> {
        TtSyncRuntime::load_paired_servers(self).await
    }

    async fn upsert_paired_server(&self, server: TtSyncPairedServer) -> Result<(), DomainError> {
        TtSyncRuntime::upsert_paired_server(self, server).await
    }

    async fn remove_paired_server(&self, server_device_id: &DeviceId) -> Result<(), DomainError> {
        TtSyncRuntime::remove_paired_server(self, server_device_id).await
    }
}

impl TtSyncRuntime {
    pub fn new(sync_root: PathBuf, store_root: PathBuf) -> Self {
        Self {
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
}
