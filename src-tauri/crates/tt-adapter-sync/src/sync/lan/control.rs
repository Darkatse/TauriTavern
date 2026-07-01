use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::sync::lan::server::{LanSyncServerHandle, spawn_lan_sync_server};
use crate::sync::lan::store::LanPeerStore;
use tt_domain::errors::DomainError;
use tt_ports::lan_sync::{
    LanInboundRequestHandler, LanServerControl, LanServerErrorReporter, LanServerInfo,
};

pub struct AxumLanServerControl {
    sync_root: PathBuf,
    store: LanPeerStore,
    inbound: Arc<dyn LanInboundRequestHandler>,
    errors: Arc<dyn LanServerErrorReporter>,
    server: Mutex<Option<LanSyncServerHandle>>,
}

impl AxumLanServerControl {
    pub fn new(
        sync_root: PathBuf,
        store: LanPeerStore,
        inbound: Arc<dyn LanInboundRequestHandler>,
        errors: Arc<dyn LanServerErrorReporter>,
    ) -> Self {
        Self {
            sync_root,
            store,
            inbound,
            errors,
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
            self.errors.clone(),
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
