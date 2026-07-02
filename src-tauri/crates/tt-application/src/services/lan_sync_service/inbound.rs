use std::sync::Arc;

use async_trait::async_trait;
use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_contract::sync::SyncMode;
use uuid::Uuid;

use super::pairing_link::{
    decode_device_pubkey_b64url, default_lan_permissions, device_pubkey_b64url,
    host_for_pairing_prompt,
};
use super::ports::{
    LanInboundRequestHandler, LanPairingApprovalRequest, LanPeerRepository,
    LanSyncSettingsRepository, PairingApproval,
};
use super::runtime_state::LanSyncRuntimeState;
use super::{PAIRING_REJECTED_MESSAGE, now_ms};
use crate::services::sync_job_coordinator::{StartedSyncJob, SyncJobCoordinator};
use crate::services::sync_policy::validate_sync_operation_options;
use tt_contracts::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin,
};
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::{
    LanPairCompleteRequest, LanPairCompleteResponse, LanSyncPairedDevice,
};

pub struct LanInboundService {
    state: Arc<LanSyncRuntimeState>,
    settings_repository: Arc<dyn LanSyncSettingsRepository>,
    peer_repository: Arc<dyn LanPeerRepository>,
    coordinator: Arc<SyncJobCoordinator>,
    approval: Arc<dyn PairingApproval>,
}

impl LanInboundService {
    pub fn new(
        state: Arc<LanSyncRuntimeState>,
        settings_repository: Arc<dyn LanSyncSettingsRepository>,
        peer_repository: Arc<dyn LanPeerRepository>,
        coordinator: Arc<SyncJobCoordinator>,
        approval: Arc<dyn PairingApproval>,
    ) -> Self {
        Self {
            state,
            settings_repository,
            peer_repository,
            coordinator,
            approval,
        }
    }

    async fn effective_sync_mode(&self) -> Result<SyncMode, DomainError> {
        Ok(self
            .state
            .get_sync_mode_override()
            .await
            .unwrap_or(self.manual_default_mode().await?))
    }

    async fn manual_default_mode(&self) -> Result<SyncMode, DomainError> {
        Ok(self
            .settings_repository
            .load_or_create_sync_preferences()
            .await?
            .manual_default_mode)
    }
}

#[async_trait]
impl LanInboundRequestHandler for LanInboundService {
    async fn complete_pairing(
        &self,
        token: String,
        request: LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError> {
        let session = self.state.active_pairing_session(&token, now_ms()).await?;

        let identity = self.peer_repository.load_or_create_identity().await?;
        if request.device_id == identity.device_id {
            return Err(DomainError::InvalidData(
                "Cannot pair LAN Sync device with itself".to_string(),
            ));
        }

        super::pairing_link::validate_https_base_url(&request.client_base_url)?;
        if request.client_spki_sha256.trim().is_empty() {
            return Err(DomainError::InvalidData(
                "Missing LAN Sync client SPKI".to_string(),
            ));
        }

        let public_key = decode_device_pubkey_b64url(&request.device_pubkey)?;
        let accepted = self
            .approval
            .request(LanPairingApprovalRequest {
                request_id: new_request_id(),
                peer_device_id: request.device_id.to_string(),
                peer_device_name: request.device_name.clone(),
                peer_ip: host_for_pairing_prompt(&request.client_base_url)?,
                expires_at_ms: session.expires_at_ms,
            })
            .await?;

        if !accepted {
            return Err(DomainError::AuthenticationError(
                PAIRING_REJECTED_MESSAGE.to_string(),
            ));
        }

        self.state.consume_pairing_session(&token, now_ms()).await?;

        let permissions = default_lan_permissions();
        self.peer_repository
            .upsert_paired_device(LanSyncPairedDevice {
                grant: PeerGrant {
                    device_id: request.device_id,
                    device_name: request.device_name,
                    public_key,
                    permissions,
                    paired_at_ms: now_ms(),
                    last_sync_ms: None,
                },
                base_url: request.client_base_url,
                spki_sha256: request.client_spki_sha256,
            })
            .await?;

        Ok(LanPairCompleteResponse {
            server_device_id: identity.device_id,
            server_device_name: identity.device_name,
            server_device_pubkey: device_pubkey_b64url(&identity.ed25519_seed)?,
            granted_permissions: permissions,
        })
    }

    async fn accept_pull_request(
        &self,
        peer_device_id: DeviceId,
        options: SyncOperationOptions,
    ) -> Result<(), DomainError> {
        let options = validate_sync_operation_options(options)?;
        let mode = self.effective_sync_mode().await?;
        let request = SyncJobRequest {
            endpoint: SyncEndpointRef::LanPeer {
                device_id: peer_device_id.clone(),
            },
            intent: SyncIntent::PullToLocal,
            origin: SyncOrigin::RemoteRequest {
                peer_id: peer_device_id,
            },
            policy: ResolvedSyncPolicy::Transfer { mode, options },
        };

        let started = self
            .coordinator
            .try_start(request)
            .map_err(|report| DomainError::InvalidData(report_failure_message(&report)))?;

        spawn_inbound_job(started);
        Ok(())
    }
}

fn spawn_inbound_job(started: StartedSyncJob) {
    tokio::spawn(async move {
        let _ = started.execute().await.finish();
    });
}

fn report_failure_message(report: &SyncJobReport) -> String {
    report
        .failure_message()
        .unwrap_or("LAN Sync job did not start")
        .to_string()
}

fn new_request_id() -> String {
    Uuid::new_v4().to_string()
}
