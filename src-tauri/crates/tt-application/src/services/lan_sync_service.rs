use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_contract::sync::{OverwritePolicy, SyncMode};

use crate::services::sync_job_coordinator::SyncJobCoordinator;
use crate::services::sync_policy::validate_sync_operation_options;
use tt_contracts::lan_discovery::{LanDiscoveredDevice, LanDiscoveryAnnouncement};
use tt_contracts::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin,
};
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::{
    LanSyncPairedDevice, LanSyncPairedDeviceSummary, LanSyncStatus, validate_device_name,
};
use tt_ports::lan_discovery::LanDeviceDiscovery;

mod inbound;
mod pairing_link;
pub mod ports;
mod runtime_state;

#[cfg(test)]
mod tests;

pub use inbound::LanInboundService;
use pairing_link::{
    build_pair_uri, decode_device_pubkey_b64url, device_pubkey_b64url, parse_device_id,
    parse_pair_uri,
};
pub use ports::{
    LanAddressDiscovery, LanInboundRequestHandler, LanPairingClient, LanPeerRepository,
    LanServerControl, LanServerInfo, LanSyncSettingsRepository, PairingApproval,
};
pub use runtime_state::LanSyncRuntimeState;
pub use tt_contracts::sync::PAIRING_REJECTED_MESSAGE;

pub struct LanSyncService {
    state: Arc<LanSyncRuntimeState>,
    settings_repository: Arc<dyn LanSyncSettingsRepository>,
    peer_repository: Arc<dyn LanPeerRepository>,
    server: Arc<dyn LanServerControl>,
    addresses: Arc<dyn LanAddressDiscovery>,
    discovery: Arc<dyn LanDeviceDiscovery>,
    pairing_client: Arc<dyn LanPairingClient>,
    approval: Arc<dyn PairingApproval>,
    coordinator: Arc<SyncJobCoordinator>,
}

impl LanSyncService {
    #[expect(
        clippy::too_many_arguments,
        reason = "composition boundary keeps independent LAN service dependencies explicit"
    )]
    pub fn new(
        state: Arc<LanSyncRuntimeState>,
        settings_repository: Arc<dyn LanSyncSettingsRepository>,
        peer_repository: Arc<dyn LanPeerRepository>,
        server: Arc<dyn LanServerControl>,
        addresses: Arc<dyn LanAddressDiscovery>,
        discovery: Arc<dyn LanDeviceDiscovery>,
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
            discovery,
            pairing_client,
            approval,
            coordinator,
        }
    }

    pub async fn get_status(&self) -> Result<LanSyncStatus, DomainError> {
        let identity = self.peer_repository.load_or_create_identity().await?;
        let settings = self
            .settings_repository
            .load_or_create_server_settings()
            .await?;
        let (sync_mode, manual_default_mode, sync_mode_overridden, overwrite_policy) =
            self.sync_preference_state().await?;

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
            device_name: identity.device_name,
            running,
            address,
            available_addresses,
            port,
            sync_mode,
            manual_default_mode,
            sync_mode_overridden,
            overwrite_policy,
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

    pub async fn set_overwrite_policy(
        &self,
        overwrite_policy: OverwritePolicy,
    ) -> Result<(), DomainError> {
        let mut preferences = self
            .settings_repository
            .load_or_create_sync_preferences()
            .await?;
        preferences.overwrite_policy = overwrite_policy;
        self.settings_repository
            .save_sync_preferences(&preferences)
            .await
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

    async fn sync_preference_state(
        &self,
    ) -> Result<(SyncMode, SyncMode, bool, OverwritePolicy), DomainError> {
        let preferences = self
            .settings_repository
            .load_or_create_sync_preferences()
            .await?;
        let sync_mode_override = self.state.get_sync_mode_override().await;
        let sync_mode_overridden = sync_mode_override.is_some();
        Ok((
            sync_mode_override.unwrap_or(preferences.manual_default_mode),
            preferences.manual_default_mode,
            sync_mode_overridden,
            preferences.overwrite_policy,
        ))
    }

    async fn ensure_server_running(&self) -> Result<LanServerInfo, DomainError> {
        self.server
            .running_info()
            .await
            .ok_or_else(|| DomainError::InvalidData("LAN Sync server is not running".to_string()))
    }

    pub async fn get_pairing_info(&self) -> Result<LanSyncPairingInfo, DomainError> {
        let server_info = self.ensure_server_running().await?;
        let addresses = self.addresses.list_available_addresses(server_info.port)?;
        let address = self
            .addresses
            .default_advertise_address(server_info.port, &addresses)
            .ok_or_else(|| {
                DomainError::InvalidData("No available LAN sync addresses".to_string())
            })?;
        let identity = self.peer_repository.load_or_create_identity().await?;
        Ok(LanSyncPairingInfo {
            pair_uri: build_pair_uri(&address, &identity.device_id, &server_info.spki_sha256)?,
            address,
        })
    }

    pub async fn discover_devices(&self) -> Result<Vec<LanDiscoveredDevice>, DomainError> {
        self.discovery.discover_devices().await
    }

    pub async fn set_device_name(&self, name: &str) -> Result<(), DomainError> {
        let name = name.trim();
        validate_device_name(name)?;
        self.peer_repository.set_device_name(name).await?;
        if let Err(error) = self.discovery.set_device_name(name).await {
            tracing::warn!("Device name saved; LAN discovery announcement failed: {error}");
        }
        Ok(())
    }

    pub async fn connect_address(&self, address: &str) -> Result<(), DomainError> {
        let base_url = pairing_link::parse_manual_address(address)?;
        let mut device = self.pairing_client.probe_device(&base_url, None).await?;
        let identity = self.peer_repository.load_or_create_identity().await?;
        if device.device_id == identity.device_id {
            return Err(DomainError::InvalidData(
                "Cannot pair LAN Sync device with itself".to_string(),
            ));
        }
        if let Some(peer) = self
            .peer_repository
            .load_paired_devices()
            .await?
            .into_iter()
            .find(|peer| peer.grant.device_id == device.device_id)
        {
            // A manually supplied address never replaces an existing trust decision.
            device = self
                .pairing_client
                .probe_device(&base_url, Some(&peer.spki_sha256))
                .await?;
            if device.device_id != peer.grant.device_id {
                return Err(DomainError::AuthenticationError(
                    "LAN Sync device identity does not match".to_string(),
                ));
            }
            return self
                .peer_repository
                .update_paired_connection(
                    &device.device_id,
                    &base_url,
                    &device.device_name,
                    device.platform.as_deref(),
                )
                .await;
        }
        self.request_pairing_with_peer(vec![base_url], device.spki_sha256, Some(device.device_id))
            .await?;
        Ok(())
    }

    pub async fn pair_device(
        &self,
        device_id: &str,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        let device_id = parse_device_id(device_id)?;
        let device = self
            .discover_devices()
            .await?
            .into_iter()
            .find(|device| device.device_id == device_id)
            .ok_or_else(|| {
                DomainError::NotFound(
                    "Device is no longer nearby. Refresh and try again.".to_string(),
                )
            })?;
        self.request_pairing_with_peer(device.base_urls, device.spki_sha256, Some(device_id))
            .await
    }

    pub async fn request_pairing(
        &self,
        pair_uri: &str,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        let parsed = parse_pair_uri(pair_uri)?;
        let mut addresses = Vec::new();
        if let Some(device_id) = &parsed.device_id {
            // A link remains usable when multicast is unavailable.
            match self.discover_devices().await {
                Ok(devices) => {
                    if let Some(device) = devices.into_iter().find(|device| {
                        &device.device_id == device_id && device.spki_sha256 == parsed.spki_sha256
                    }) {
                        addresses = device.base_urls;
                    }
                }
                Err(error) => tracing::warn!("LAN discovery unavailable while pairing: {error}"),
            }
        }
        if !addresses.contains(&parsed.base_url) {
            addresses.push(parsed.base_url);
        }
        self.request_pairing_with_peer(addresses, parsed.spki_sha256, parsed.device_id)
            .await
    }

    async fn request_pairing_with_peer(
        &self,
        addresses: Vec<String>,
        spki_sha256: String,
        expected_device_id: Option<DeviceId>,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        let identity = self.peer_repository.load_or_create_identity().await?;
        if expected_device_id.as_ref() == Some(&identity.device_id) {
            return Err(DomainError::InvalidData(
                "Cannot pair LAN Sync device with itself".to_string(),
            ));
        }
        if self.server.running_info().await.is_none() {
            self.start_server().await?;
        }
        let server_info = self.ensure_server_running().await?;
        let local_device = LanDiscoveryAnnouncement {
            device_id: identity.device_id.clone(),
            device_name: identity.device_name.clone(),
            platform: Some(identity.platform),
            port: server_info.port,
            spki_sha256: server_info.spki_sha256,
        };
        let (response, base_url) = self
            .pairing_client
            .complete_pairing(
                &addresses,
                &spki_sha256,
                expected_device_id.as_ref(),
                &local_device,
                &device_pubkey_b64url(&identity.ed25519_seed)?,
            )
            .await?;
        if response.server_device_id == identity.device_id {
            return Err(DomainError::InvalidData(
                "Cannot pair LAN Sync device with itself".to_string(),
            ));
        }
        if expected_device_id
            .as_ref()
            .is_some_and(|expected| expected != &response.server_device_id)
        {
            return Err(DomainError::AuthenticationError(
                "LAN Sync device identity does not match".to_string(),
            ));
        }
        let paired_device = LanSyncPairedDevice {
            platform: response.server_device_platform,
            grant: PeerGrant {
                device_id: response.server_device_id,
                device_name: response.server_device_name,
                public_key: decode_device_pubkey_b64url(&response.server_device_pubkey)?,
                permissions: response.granted_permissions,
                paired_at_ms: now_ms(),
                last_sync_ms: None,
            },
            base_url,
            spki_sha256,
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
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
