use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, oneshot};

use tt_contracts::sync::SyncJobEvent;
use tt_contracts::sync_automation::{SyncAutomationStatus, SyncAutomationToastEvent};
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::LanSyncPairRequestEvent;
use tt_ports::lan_sync::{LanPairingApprovalRequest, LanServerEvents, PairingApproval};
use tt_ports::sync::SyncJobEventPublisher;
use tt_ports::sync_automation::SyncAutomationEventPublisher;

pub(crate) fn sync_automation_events(
    app_handle: &AppHandle,
) -> Arc<dyn SyncAutomationEventPublisher> {
    Arc::new(TauriSyncAutomationEventPublisher {
        app_handle: app_handle.clone(),
    })
}

pub(crate) fn sync_job_events(app_handle: &AppHandle) -> Arc<dyn SyncJobEventPublisher> {
    Arc::new(TauriSyncJobEventPublisher {
        app_handle: app_handle.clone(),
    })
}

pub(crate) fn pairing_approval(app_handle: &AppHandle) -> Arc<dyn PairingApproval> {
    Arc::new(TauriPairingApproval::new(app_handle.clone()))
}

pub(crate) fn lan_server_events(app_handle: &AppHandle) -> Arc<dyn LanServerEvents> {
    Arc::new(TauriLanServerEvents {
        app_handle: app_handle.clone(),
    })
}

struct TauriSyncAutomationEventPublisher {
    app_handle: AppHandle,
}

impl SyncAutomationEventPublisher for TauriSyncAutomationEventPublisher {
    fn publish_status(&self, status: SyncAutomationStatus) {
        if let Err(error) = self.app_handle.emit("sync_auto:status", status) {
            tracing::warn!("Failed to emit sync automation status: {}", error);
        }
    }

    fn publish_toast(&self, event: SyncAutomationToastEvent) {
        if let Err(error) = self.app_handle.emit("sync_auto:toast", event) {
            tracing::warn!("Failed to emit sync automation toast: {}", error);
        }
    }
}

struct TauriSyncJobEventPublisher {
    app_handle: AppHandle,
}

impl SyncJobEventPublisher for TauriSyncJobEventPublisher {
    fn publish_sync_job(&self, event: SyncJobEvent) {
        if let Err(error) = self.app_handle.emit("sync:job", event) {
            tracing::warn!("Failed to emit Sync job event: {}", error);
        }
    }
}

struct TauriLanServerEvents {
    app_handle: AppHandle,
}

impl LanServerEvents for TauriLanServerEvents {
    fn report_lan_server_error(&self, message: String) {
        tracing::error!(
            target: crate::observability_targets::USER_VISIBLE_ERROR,
            "{message}"
        );
    }

    fn pairing_completed(&self) {
        if let Err(error) = self.app_handle.emit("lan_sync:pairing_completed", ()) {
            tracing::warn!("Failed to publish completed LAN pairing: {error}");
        }
    }
}

pub(crate) fn lan_discovery_host(
    app_handle: &AppHandle,
) -> Arc<dyn tt_ports::lan_discovery::LanDiscoveryHost> {
    Arc::new(TauriLanDiscoveryHost {
        app_handle: app_handle.clone(),
    })
}

struct TauriLanDiscoveryHost {
    app_handle: AppHandle,
}

#[async_trait]
impl tt_ports::lan_discovery::LanDiscoveryHost for TauriLanDiscoveryHost {
    async fn set_enabled(&self, enabled: bool) -> Result<(), DomainError> {
        crate::platform::lan_discovery::set_lan_discovery_enabled(&self.app_handle, enabled).await
    }

    fn publish_changed(&self, devices: Vec<tt_contracts::lan_discovery::LanDiscoveredDevice>) {
        if let Err(error) = self.app_handle.emit("lan_sync:devices_changed", devices) {
            tracing::warn!("Failed to publish nearby LAN devices: {error}");
        }
    }
}

struct TauriPairingApproval {
    app_handle: AppHandle,
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl TauriPairingApproval {
    fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl PairingApproval for TauriPairingApproval {
    async fn request(&self, request: LanPairingApprovalRequest) -> Result<bool, DomainError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request.request_id.clone(), tx);
        }

        if let Err(error) = self.app_handle.emit(
            "lan_sync:pair_request",
            LanSyncPairRequestEvent {
                request_id: request.request_id.clone(),
                peer_device_id: request.peer_device_id,
                peer_device_name: request.peer_device_name,
                peer_ip: request.peer_ip,
            },
        ) {
            self.pending.lock().await.remove(&request.request_id);
            return Err(DomainError::InternalError(error.to_string()));
        }

        let timeout = Duration::from_millis(request.expires_at_ms.saturating_sub(now_ms()));
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(accepted)) => Ok(accepted),
            Ok(Err(_)) => Err(DomainError::cancelled("Pairing request cancelled")),
            Err(_) => {
                self.pending.lock().await.remove(&request.request_id);
                Err(DomainError::AuthenticationError(
                    "Pairing expired".to_string(),
                ))
            }
        }
    }

    async fn confirm(&self, request_id: &str, accept: bool) -> Result<(), DomainError> {
        let tx = self
            .pending
            .lock()
            .await
            .remove(request_id)
            .ok_or_else(|| {
                DomainError::NotFound(format!("Pair request not found: {}", request_id))
            })?;

        tx.send(accept).map_err(|_| {
            DomainError::InternalError("Pairing decision receiver dropped".to_string())
        })
    }

    async fn cancel_all(&self) {
        self.pending.lock().await.clear();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
