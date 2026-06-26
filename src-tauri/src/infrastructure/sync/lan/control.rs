use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::application::services::lan_sync_service::ports::{
    LanInboundRequestHandler, LanServerControl, LanServerInfo,
};
use crate::domain::errors::DomainError;
use crate::infrastructure::sync::lan::server::{LanSyncServerHandle, spawn_lan_sync_server};
use crate::infrastructure::sync::lan::store::LanPeerStore;

pub struct AxumLanServerControl {
    sync_root: PathBuf,
    store: LanPeerStore,
    inbound: Arc<dyn LanInboundRequestHandler>,
    server: Mutex<Option<LanSyncServerHandle>>,
}

impl AxumLanServerControl {
    pub fn new(
        sync_root: PathBuf,
        store: LanPeerStore,
        inbound: Arc<dyn LanInboundRequestHandler>,
    ) -> Self {
        Self {
            sync_root,
            store,
            inbound,
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
        let handle = spawn_lan_sync_server(
            addr,
            self.sync_root.clone(),
            self.store.clone(),
            self.inbound.clone(),
        )
        .await?;
        let info = handle.info();
        *server = Some(handle);
        Ok(info)
    }

    async fn stop(&self) -> Result<(), DomainError> {
        let handle = self.server.lock().await.take();
        if let Some(handle) = handle {
            handle.shutdown();
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
