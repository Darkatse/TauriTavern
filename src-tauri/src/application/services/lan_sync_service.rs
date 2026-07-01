use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_contract::sync::SyncMode;

use crate::application::services::sync_job_coordinator::SyncJobCoordinator;
use crate::application::services::sync_policy::validate_sync_operation_options;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanPairCompleteRequest, LanSyncPairedDevice, LanSyncPairedDeviceSummary, LanSyncStatus,
};
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin,
};

mod inbound;
mod pairing_link;
pub mod ports;
mod runtime_state;

#[cfg(test)]
mod tests;

pub use inbound::LanInboundService;
use pairing_link::{
    ParsedPairUri, build_pair_uri, decode_device_pubkey_b64url, device_pubkey_b64url,
    parse_device_id, parse_pair_uri, validate_https_base_url,
};
pub use ports::{
    LanAddressDiscovery, LanInboundRequestHandler, LanPairingApprovalRequest, LanPairingClient,
    LanPeerRepository, LanServerControl, LanServerInfo, LanSyncSettingsRepository, PairingApproval,
};
pub use runtime_state::{LanPairingSession, LanSyncRuntimeState};
pub use tt_contracts::sync::PAIRING_REJECTED_MESSAGE;

pub struct LanSyncService {
    state: Arc<LanSyncRuntimeState>,
    settings_repository: Arc<dyn LanSyncSettingsRepository>,
    peer_repository: Arc<dyn LanPeerRepository>,
    server: Arc<dyn LanServerControl>,
    addresses: Arc<dyn LanAddressDiscovery>,
    pairing_client: Arc<dyn LanPairingClient>,
    approval: Arc<dyn PairingApproval>,
    coordinator: Arc<SyncJobCoordinator>,
}

impl LanSyncService {
    pub fn new(
        state: Arc<LanSyncRuntimeState>,
        settings_repository: Arc<dyn LanSyncSettingsRepository>,
        peer_repository: Arc<dyn LanPeerRepository>,
        server: Arc<dyn LanServerControl>,
        addresses: Arc<dyn LanAddressDiscovery>,
        pairing_client: Arc<dyn LanPairingClient>,
        approval: Arc<dyn PairingApproval>,
        coordinator: Arc<SyncJobCoordinator>,
    ) -> Self {
        Self {
            state,
            settings_repository,
            peer_repository,
            server,
            addresses,
            pairing_client,
            approval,
            coordinator,
        }
    }

    pub async fn get_status(&self) -> Result<LanSyncStatus, DomainError> {
        let settings = self
            .settings_repository
            .load_or_create_server_settings()
            .await?;
        let (sync_mode, manual_default_mode, sync_mode_overridden) = self.sync_mode_state().await?;

        let pairing = self.state.get_pairing_session().await;
        let now_ms = now_ms();

        let pairing_enabled = pairing
            .as_ref()
            .is_some_and(|session| session.expires_at_ms > now_ms);
        let pairing_expires_at_ms = pairing.as_ref().map(|session| session.expires_at_ms);

        let running_info = self.server.running_info().await;
        let (running, port) = match running_info.as_ref() {
            Some(info) => (true, info.port),
            None => (false, settings.port),
        };

        let available_addresses = self.addresses.list_available_addresses(port)?;
        let address = self
            .addresses
            .default_advertise_address(port, &available_addresses);
        Ok(LanSyncStatus {
            running,
            address,
            available_addresses,
            port,
            pairing_enabled,
            pairing_expires_at_ms,
            sync_mode,
            manual_default_mode,
            sync_mode_overridden,
        })
    }

    pub async fn start_server(&self) -> Result<LanSyncStatus, DomainError> {
        let settings = self
            .settings_repository
            .load_or_create_server_settings()
            .await?;
        let _ = self.server.start(settings.port).await?;
        self.get_status().await
    }

    pub async fn stop_server(&self) -> Result<(), DomainError> {
        self.server.stop().await?;
        self.state.clear_pairing_session().await;
        self.approval.cancel_all().await;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), DomainError> {
        self.stop_server().await
    }

    pub async fn set_sync_mode(&self, mode: SyncMode, persist: bool) -> Result<(), DomainError> {
        if persist {
            let mut preferences = self
                .settings_repository
                .load_or_create_sync_preferences()
                .await?;
            preferences.manual_default_mode = mode;
            self.settings_repository
                .save_sync_preferences(&preferences)
                .await?;
            self.state.set_sync_mode_override(None).await;
            return Ok(());
        }

        self.state.set_sync_mode_override(Some(mode)).await;
        Ok(())
    }

    pub async fn clear_sync_mode_override(&self) {
        self.state.set_sync_mode_override(None).await;
    }

    pub async fn effective_sync_mode(&self) -> Result<SyncMode, DomainError> {
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

    async fn sync_mode_state(&self) -> Result<(SyncMode, SyncMode, bool), DomainError> {
        let manual_default_mode = self.manual_default_mode().await?;
        let sync_mode_override = self.state.get_sync_mode_override().await;
        let sync_mode_overridden = sync_mode_override.is_some();
        Ok((
            sync_mode_override.unwrap_or(manual_default_mode),
            manual_default_mode,
            sync_mode_overridden,
        ))
    }

    async fn ensure_server_running(&self) -> Result<LanServerInfo, DomainError> {
        self.server
            .running_info()
            .await
            .ok_or_else(|| DomainError::InvalidData("LAN Sync server is not running".to_string()))
    }

    pub async fn enable_pairing(
        &self,
        advertise_address: Option<String>,
    ) -> Result<LanSyncPairingInfo, DomainError> {
        let server_info = self.ensure_server_running().await?;

        let address = match advertise_address {
            Some(value) => {
                validate_https_base_url(&value)?;
                value
            }
            None => {
                let available_addresses =
                    self.addresses.list_available_addresses(server_info.port)?;
                self.addresses
                    .default_advertise_address(server_info.port, &available_addresses)
                    .ok_or_else(|| {
                        DomainError::InvalidData("No available LAN sync addresses".to_string())
                    })?
            }
        };

        let expires_at_ms = now_ms() + 5 * 60 * 1000;
        let token = ttsync_core::crypto::random_base64url(16);

        self.state
            .set_pairing_session(LanPairingSession {
                token: token.clone(),
                expires_at_ms,
            })
            .await;

        Ok(LanSyncPairingInfo {
            address: address.clone(),
            pair_uri: build_pair_uri(&address, &token, expires_at_ms, &server_info.spki_sha256)?,
            expires_at_ms,
        })
    }

    pub async fn get_pairing_info(
        &self,
        advertise_address: &str,
    ) -> Result<LanSyncPairingInfo, DomainError> {
        let server_info = self.ensure_server_running().await?;
        validate_https_base_url(advertise_address)?;

        let session = self.state.get_pairing_session().await.ok_or_else(|| {
            DomainError::InvalidData("LAN sync pairing is not enabled".to_string())
        })?;

        if now_ms() > session.expires_at_ms {
            return Err(DomainError::InvalidData(
                "LAN sync pairing expired".to_string(),
            ));
        }

        Ok(LanSyncPairingInfo {
            address: advertise_address.to_string(),
            pair_uri: build_pair_uri(
                advertise_address,
                &session.token,
                session.expires_at_ms,
                &server_info.spki_sha256,
            )?,
            expires_at_ms: session.expires_at_ms,
        })
    }

    pub async fn request_pairing(
        &self,
        pair_uri: &str,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        let parsed = parse_pair_uri(pair_uri)?;
        self.request_pairing_with_peer(parsed).await
    }

    async fn request_pairing_with_peer(
        &self,
        parsed: ParsedPairUri,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        if now_ms() > parsed.expires_at_ms {
            return Err(DomainError::InvalidData(
                "LAN Sync pairing expired".to_string(),
            ));
        }

        let server_info = self.ensure_server_running().await.map_err(|_| {
            DomainError::InvalidData("LAN sync server must be running before pairing".to_string())
        })?;
        let local_base_url = self
            .addresses
            .routed_advertise_address(&parsed.base_url, server_info.port)
            .await?;

        let identity = self.peer_repository.load_or_create_identity().await?;
        let request = LanPairCompleteRequest {
            device_id: identity.device_id.clone(),
            device_name: identity.device_name.clone(),
            device_pubkey: device_pubkey_b64url(&identity.ed25519_seed)?,
            client_base_url: local_base_url,
            client_spki_sha256: server_info.spki_sha256,
        };

        let response = self
            .pairing_client
            .complete_pairing(
                &parsed.base_url,
                &parsed.spki_sha256,
                &parsed.token,
                &request,
            )
            .await?;

        if response.server_device_id == identity.device_id {
            return Err(DomainError::InvalidData(
                "Cannot pair LAN Sync device with itself".to_string(),
            ));
        }

        let paired_device = LanSyncPairedDevice {
            grant: PeerGrant {
                device_id: response.server_device_id,
                device_name: response.server_device_name,
                public_key: decode_device_pubkey_b64url(&response.server_device_pubkey)?,
                permissions: response.granted_permissions,
                paired_at_ms: now_ms(),
                last_sync_ms: None,
            },
            base_url: parsed.base_url,
            spki_sha256: parsed.spki_sha256,
        };

        self.peer_repository
            .upsert_paired_device(paired_device.clone())
            .await?;

        Ok(paired_device.into())
    }

    pub async fn confirm_pairing(&self, request_id: &str, accept: bool) -> Result<(), DomainError> {
        self.approval.confirm(request_id, accept).await
    }

    pub async fn list_paired_devices(
        &self,
    ) -> Result<Vec<LanSyncPairedDeviceSummary>, DomainError> {
        Ok(self
            .peer_repository
            .load_paired_devices()
            .await?
            .into_iter()
            .map(LanSyncPairedDeviceSummary::from)
            .collect())
    }

    pub async fn remove_paired_device(&self, device_id: &str) -> Result<(), DomainError> {
        let device_id = parse_device_id(device_id)?;
        self.peer_repository
            .remove_paired_device(&device_id)
            .await?;
        Ok(())
    }

    pub async fn sync_from_device(
        &self,
        device_id: &str,
        options: SyncOperationOptions,
    ) -> Result<SyncJobReport, DomainError> {
        let device_id = parse_device_id(device_id)?;
        let options = validate_sync_operation_options(options)?;
        let mode = self.effective_sync_mode().await?;
        let request = self.job_request(
            SyncEndpointRef::LanPeer { device_id },
            SyncIntent::PullToLocal,
            SyncOrigin::Manual,
            mode,
            options,
        );
        Ok(self.coordinator.run(request).await)
    }

    pub async fn push_to_device(
        &self,
        device_id: &str,
        options: SyncOperationOptions,
    ) -> Result<SyncJobReport, DomainError> {
        self.ensure_server_running().await?;
        let device_id = parse_device_id(device_id)?;
        let options = validate_sync_operation_options(options)?;
        self.run_request_remote_pull(device_id, SyncOrigin::Manual, options)
            .await
    }

    async fn run_request_remote_pull(
        &self,
        device_id: DeviceId,
        origin: SyncOrigin,
        options: SyncOperationOptions,
    ) -> Result<SyncJobReport, DomainError> {
        let request = SyncJobRequest {
            endpoint: SyncEndpointRef::LanPeer { device_id },
            intent: SyncIntent::ReplicateLocalToRemote,
            origin,
            policy: ResolvedSyncPolicy::RemotePullRequest { options },
        };
        match self.coordinator.try_start(request) {
            Ok(started) => started.execute().await.finish_or_error(),
            Err(report) => Err(DomainError::InvalidData(
                report
                    .failure_message()
                    .unwrap_or("Sync job already running")
                    .to_string(),
            )),
        }
    }

    fn job_request(
        &self,
        endpoint: SyncEndpointRef,
        intent: SyncIntent,
        origin: SyncOrigin,
        mode: SyncMode,
        options: SyncOperationOptions,
    ) -> SyncJobRequest {
        SyncJobRequest {
            endpoint,
            intent,
            origin,
            policy: ResolvedSyncPolicy::Transfer { mode, options },
        }
    }
}

pub struct LanSyncPairingInfo {
    pub address: String,
    pub pair_uri: String,
    pub expires_at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
