use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::sync::lan::peer_discovery::LanPeerDiscovery;
use crate::sync::lan::server::{LanSyncServerHandle, spawn_lan_sync_server};
use crate::sync::lan::store::LanPeerStore;
use tt_contracts::lan_discovery::LanDiscoveryAnnouncement;
use tt_domain::errors::DomainError;
use tt_ports::lan_sync::{
    LanInboundRequestHandler, LanServerControl, LanServerEvents, LanServerInfo,
};

pub struct AxumLanServerControl {
    sync_root: PathBuf,
    store: LanPeerStore,
    inbound: Arc<dyn LanInboundRequestHandler>,
    events: Arc<dyn LanServerEvents>,
    discovery: LanPeerDiscovery,
    server: Mutex<Option<LanSyncServerHandle>>,
}

impl AxumLanServerControl {
    pub fn new(
        sync_root: PathBuf,
        store: LanPeerStore,
        inbound: Arc<dyn LanInboundRequestHandler>,
        events: Arc<dyn LanServerEvents>,
        discovery: LanPeerDiscovery,
    ) -> Self {
        Self {
            sync_root,
            store,
            inbound,
            events,
            discovery,
            server: Mutex::new(None),
        }
    }
}

#[async_trait]
impl LanServerControl for AxumLanServerControl {
    async fn start(&self, port: u16) -> Result<LanServerInfo, DomainError> {
        let mut server = self.server.lock().await;
        if let Some(handle) = server.as_ref() {
            return Ok(handle.info());
        }

        let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
        let identity = self.store.load_or_create_identity().await?;
        let handle = spawn_lan_sync_server(
            addr,
            self.sync_root.clone(),
            self.store.clone(),
            self.inbound.clone(),
            self.events.clone(),
        )
        .await?;
        let info = handle.info();
        *server = Some(handle);
        if let Err(error) = self
            .discovery
            .set_local_device(LanDiscoveryAnnouncement {
                device_id: identity.device_id,
                device_name: identity.device_name,
                platform: Some(identity.platform),
                port: info.port,
                spki_sha256: info.spki_sha256.clone(),
            })
            .await
        {
            tracing::warn!("LAN HTTPS server is running, but discovery is unavailable: {error}");
        }
        Ok(info)
    }

    async fn stop(&self) -> Result<(), DomainError> {
        let handle = self.server.lock().await.take();
        if let Some(handle) = handle {
            handle.shutdown();
        }
        if let Err(error) = self.discovery.stop().await {
            tracing::warn!("Failed to release LAN discovery resources: {error}");
        }
        Ok(())
    }

    async fn running_info(&self) -> Option<LanServerInfo> {
        self.server
            .lock()
            .await
            .as_ref()
            .map(LanSyncServerHandle::info)
    }
}
