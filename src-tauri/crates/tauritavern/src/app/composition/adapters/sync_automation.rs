use std::sync::Arc;

use async_trait::async_trait;
use ttsync_contract::sync::SyncMode;

use tt_application::services::lan_sync_service::LanSyncService;
use tt_application::services::tt_sync_service::TtSyncService;
use tt_contracts::sync_automation::SyncAutomationTarget;
use tt_domain::errors::DomainError;
use tt_ports::sync_automation::{SyncAutomationEndpointCatalog, SyncAutomationLanServerControl};

pub(in crate::app::composition) fn sync_automation_lan_server(
    lan_sync_service: Arc<LanSyncService>,
    lan_sync_allowed: bool,
) -> Arc<dyn SyncAutomationLanServerControl> {
    Arc::new(ServiceSyncAutomationLanServerControl {
        lan_sync_service,
        lan_sync_allowed,
    })
}

pub(in crate::app::composition) fn sync_automation_endpoint_catalog(
    lan_sync_service: Arc<LanSyncService>,
    tt_sync_service: Arc<TtSyncService>,
    lan_sync_allowed: bool,
) -> Arc<dyn SyncAutomationEndpointCatalog> {
    Arc::new(ServiceSyncAutomationEndpointCatalog {
        lan_sync_service,
        tt_sync_service,
        lan_sync_allowed,
    })
}

struct ServiceSyncAutomationLanServerControl {
    lan_sync_service: Arc<LanSyncService>,
    lan_sync_allowed: bool,
}

#[async_trait]
impl SyncAutomationLanServerControl for ServiceSyncAutomationLanServerControl {
    fn validate_allowed(&self) -> Result<(), DomainError> {
        if !self.lan_sync_allowed {
            return Err(DomainError::InvalidData(
                "LAN Sync is not allowed by the current platform policy".to_string(),
            ));
        }
        Ok(())
    }

    async fn start(&self) -> Result<(), DomainError> {
        self.validate_allowed()?;
        self.lan_sync_service.start_server().await.map(|_| ())
    }

    async fn ensure_running(&self) -> Result<(), DomainError> {
        self.validate_allowed()?;
        if !self.lan_sync_service.get_status().await?.running {
            return Err(DomainError::InvalidData(
                "LAN Sync server is not running. Start the sync port before using LAN auto upload."
                    .to_string(),
            ));
        }
        Ok(())
    }
}

struct ServiceSyncAutomationEndpointCatalog {
    lan_sync_service: Arc<LanSyncService>,
    tt_sync_service: Arc<TtSyncService>,
    lan_sync_allowed: bool,
}

#[async_trait]
impl SyncAutomationEndpointCatalog for ServiceSyncAutomationEndpointCatalog {
    async fn validate_target(
        &self,
        target: &SyncAutomationTarget,
        mode: SyncMode,
    ) -> Result<(), DomainError> {
        match target {
            SyncAutomationTarget::Lan { device_id } => {
                if !self.lan_sync_allowed {
                    return Err(DomainError::InvalidData(
                        "LAN Sync is not allowed by the current platform policy".to_string(),
                    ));
                }

                let devices = self.lan_sync_service.list_paired_devices().await?;
                devices
                    .iter()
                    .find(|device| device.device_id == *device_id)
                    .ok_or_else(|| {
                        DomainError::NotFound(format!("LAN Sync device not found: {device_id}"))
                    })?;
            }
            SyncAutomationTarget::Tt { server_device_id } => {
                let servers = self.tt_sync_service.list_servers().await?;
                let server = servers
                    .iter()
                    .find(|server| server.server_device_id.as_str() == server_device_id.as_str())
                    .ok_or_else(|| {
                        DomainError::NotFound(format!(
                            "TT-Sync server not found: {server_device_id}"
                        ))
                    })?;
                if !server.permissions.write {
                    return Err(DomainError::AuthenticationError(
                        "TT-Sync server does not grant write permission".to_string(),
                    ));
                }
                if mode == SyncMode::Mirror && !server.permissions.mirror_delete {
                    return Err(DomainError::AuthenticationError(
                        "TT-Sync server does not grant mirror_delete permission".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}
