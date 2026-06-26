use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use tokio::sync::Mutex;
use ttsync_contract::peer::{DeviceId, PeerGrant, Permissions};
use ttsync_contract::sync::SyncMode;
use url::Url;
use uuid::Uuid;

use crate::application::services::sync_job_coordinator::{StartedSyncJob, SyncJobCoordinator};
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanPairCompleteRequest, LanPairCompleteResponse, LanServerSettings, LanSyncPairedDevice,
    LanSyncPairedDeviceSummary, LanSyncStatus, LanSyncSyncCompletedEvent, LanSyncSyncErrorEvent,
    LanSyncSyncProgressEvent, SyncPreferences,
};
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin, resolve_sync_options,
};

pub const PAIRING_REJECTED_MESSAGE: &str = "Pairing rejected";

#[derive(Debug, Clone)]
pub struct LanPairingSession {
    pub token: String,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct LanServerInfo {
    pub port: u16,
    pub spki_sha256: String,
}

#[derive(Debug, Clone)]
pub struct LanPairingApprovalRequest {
    pub request_id: String,
    pub peer_device_id: String,
    pub peer_device_name: String,
    pub peer_ip: String,
    pub expires_at_ms: u64,
}

#[async_trait]
pub trait LanSyncSettingsRepository: Send + Sync {
    async fn load_or_create_server_settings(&self) -> Result<LanServerSettings, DomainError>;
    async fn load_or_create_sync_preferences(&self) -> Result<SyncPreferences, DomainError>;
    async fn save_sync_preferences(&self, preferences: &SyncPreferences)
    -> Result<(), DomainError>;
}

#[async_trait]
pub trait LanPeerRepository: Send + Sync {
    async fn load_or_create_identity(
        &self,
    ) -> Result<crate::domain::models::lan_sync::LanSyncIdentity, DomainError>;
    async fn load_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError>;
    async fn upsert_paired_device(&self, device: LanSyncPairedDevice) -> Result<(), DomainError>;
    async fn remove_paired_device(&self, device_id: &DeviceId) -> Result<(), DomainError>;
}

#[async_trait]
pub trait LanServerControl: Send + Sync {
    async fn start(&self, port: u16) -> Result<LanServerInfo, DomainError>;
    async fn stop(&self) -> Result<(), DomainError>;
    async fn running_info(&self) -> Option<LanServerInfo>;
}

#[async_trait]
pub trait LanAddressDiscovery: Send + Sync {
    fn list_available_addresses(&self, port: u16) -> Result<Vec<String>, DomainError>;
    fn default_advertise_address(
        &self,
        port: u16,
        available_addresses: &[String],
    ) -> Option<String>;
    async fn routed_advertise_address(
        &self,
        peer_base_url: &str,
        local_port: u16,
    ) -> Result<String, DomainError>;
}

#[async_trait]
pub trait LanPairingClient: Send + Sync {
    async fn complete_pairing(
        &self,
        base_url: &str,
        spki_sha256: &str,
        token: &str,
        request: &LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError>;
}

#[async_trait]
pub trait PairingApproval: Send + Sync {
    async fn request(&self, request: LanPairingApprovalRequest) -> Result<bool, DomainError>;
    async fn confirm(&self, request_id: &str, accept: bool) -> Result<(), DomainError>;
    async fn cancel_all(&self);
}

pub trait LanSyncEventPublisher: Send + Sync {
    fn publish_progress(&self, payload: LanSyncSyncProgressEvent);
    fn publish_completed(&self, payload: LanSyncSyncCompletedEvent);
    fn publish_error(&self, payload: LanSyncSyncErrorEvent);
}

#[async_trait]
pub trait LanInboundRequestHandler: Send + Sync {
    async fn complete_pairing(
        &self,
        token: String,
        request: LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError>;

    async fn accept_pull_request(
        &self,
        peer_device_id: DeviceId,
        options: SyncOperationOptions,
    ) -> Result<(), DomainError>;
}

pub struct LanSyncRuntimeState {
    pairing_session: Mutex<Option<LanPairingSession>>,
    sync_mode_override: Mutex<Option<SyncMode>>,
}

impl LanSyncRuntimeState {
    pub fn new() -> Self {
        Self {
            pairing_session: Mutex::new(None),
            sync_mode_override: Mutex::new(None),
        }
    }

    pub async fn set_pairing_session(&self, session: LanPairingSession) {
        *self.pairing_session.lock().await = Some(session);
    }

    pub async fn get_pairing_session(&self) -> Option<LanPairingSession> {
        self.pairing_session.lock().await.clone()
    }

    pub async fn clear_pairing_session(&self) {
        *self.pairing_session.lock().await = None;
    }

    pub async fn active_pairing_session(
        &self,
        token: &str,
        now_ms: u64,
    ) -> Result<LanPairingSession, DomainError> {
        let session =
            self.pairing_session.lock().await.clone().ok_or_else(|| {
                DomainError::AuthenticationError("Pairing not enabled".to_string())
            })?;
        validate_pairing_session(&session, token, now_ms)?;
        Ok(session)
    }

    pub async fn consume_pairing_session(
        &self,
        token: &str,
        now_ms: u64,
    ) -> Result<(), DomainError> {
        let mut pairing_session = self.pairing_session.lock().await;
        let session = pairing_session
            .as_ref()
            .ok_or_else(|| DomainError::AuthenticationError("Pairing not enabled".to_string()))?;
        validate_pairing_session(session, token, now_ms)?;
        *pairing_session = None;
        Ok(())
    }

    pub async fn get_sync_mode_override(&self) -> Option<SyncMode> {
        *self.sync_mode_override.lock().await
    }

    pub async fn set_sync_mode_override(&self, mode: Option<SyncMode>) {
        *self.sync_mode_override.lock().await = mode;
    }
}

impl Default for LanSyncRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LanInboundService {
    state: Arc<LanSyncRuntimeState>,
    settings_repository: Arc<dyn LanSyncSettingsRepository>,
    peer_repository: Arc<dyn LanPeerRepository>,
    coordinator: Arc<SyncJobCoordinator>,
    events: Arc<dyn LanSyncEventPublisher>,
    approval: Arc<dyn PairingApproval>,
}

impl LanInboundService {
    pub fn new(
        state: Arc<LanSyncRuntimeState>,
        settings_repository: Arc<dyn LanSyncSettingsRepository>,
        peer_repository: Arc<dyn LanPeerRepository>,
        coordinator: Arc<SyncJobCoordinator>,
        events: Arc<dyn LanSyncEventPublisher>,
        approval: Arc<dyn PairingApproval>,
    ) -> Self {
        Self {
            state,
            settings_repository,
            peer_repository,
            coordinator,
            events,
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

        validate_https_base_url(&request.client_base_url)?;
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

        spawn_inbound_job(started, self.events.clone());
        Ok(())
    }
}

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
        options: Option<SyncOperationOptions>,
    ) -> Result<SyncJobReport, DomainError> {
        let device_id = parse_device_id(device_id)?;
        let options = resolve_sync_options(options)?;
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
        options: Option<SyncOperationOptions>,
    ) -> Result<SyncJobReport, DomainError> {
        self.ensure_server_running().await?;
        let device_id = parse_device_id(device_id)?;
        let options = resolve_sync_options(options)?;
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

fn spawn_inbound_job(started: StartedSyncJob, events: Arc<dyn LanSyncEventPublisher>) {
    tokio::spawn(async move {
        let report = started.execute().await.finish();
        publish_inbound_report(&*events, &report);
    });
}

fn publish_inbound_report(events: &dyn LanSyncEventPublisher, report: &SyncJobReport) {
    if let Some(summary) = report.completed_summary() {
        events.publish_completed(LanSyncSyncCompletedEvent {
            files_total: summary.files_total,
            bytes_total: summary.bytes_total,
            files_deleted: summary.files_deleted,
        });
    } else if let Some(message) = report.failure_message() {
        events.publish_error(LanSyncSyncErrorEvent {
            message: message.to_string(),
        });
    }
}

fn build_pair_uri(
    base_url: &str,
    token: &str,
    expires_at_ms: u64,
    spki_sha256: &str,
) -> Result<String, DomainError> {
    validate_https_base_url(base_url)?;

    let mut uri = Url::parse("tauritavern://lan-sync/pair")
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    uri.query_pairs_mut()
        .append_pair("v", "2")
        .append_pair("url", base_url)
        .append_pair("token", token)
        .append_pair("exp", &expires_at_ms.to_string())
        .append_pair("spki", spki_sha256);

    Ok(uri.to_string())
}

struct ParsedPairUri {
    base_url: String,
    token: String,
    expires_at_ms: u64,
    spki_sha256: String,
}

fn parse_pair_uri(pair_uri: &str) -> Result<ParsedPairUri, DomainError> {
    let uri = Url::parse(pair_uri).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if uri.scheme() != "tauritavern" || uri.host_str() != Some("lan-sync") || uri.path() != "/pair"
    {
        return Err(DomainError::InvalidData(
            "Pair URI is not a LAN Sync pairing link".to_string(),
        ));
    }

    let version = uri
        .query_pairs()
        .find_map(|(key, value)| (key == "v").then(|| value.to_string()));
    if version.as_deref() != Some("2") {
        return Err(DomainError::InvalidData(
            "LAN Sync Pair URI must be v=2".to_string(),
        ));
    }

    parse_lan_pair_uri_payload(&uri)
}

fn parse_lan_pair_uri_payload(uri: &Url) -> Result<ParsedPairUri, DomainError> {
    let mut base_url = None;
    let mut token = None;
    let mut expires_at_ms = None;
    let mut spki_sha256 = None;
    for (key, value) in uri.query_pairs() {
        match key.as_ref() {
            "url" => base_url = Some(value.to_string()),
            "token" => token = Some(value.to_string()),
            "exp" => {
                expires_at_ms = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| DomainError::InvalidData("Invalid exp".to_string()))?,
                )
            }
            "spki" => spki_sha256 = Some(value.to_string()),
            _ => {}
        }
    }

    let base_url = base_url.ok_or_else(|| DomainError::InvalidData("Missing url".to_string()))?;
    validate_https_base_url(&base_url)?;

    Ok(ParsedPairUri {
        base_url,
        token: token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| DomainError::InvalidData("Missing token".to_string()))?,
        expires_at_ms: expires_at_ms
            .ok_or_else(|| DomainError::InvalidData("Missing exp".to_string()))?,
        spki_sha256: spki_sha256
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| DomainError::InvalidData("Missing spki".to_string()))?,
    })
}

fn validate_https_base_url(value: &str) -> Result<(), DomainError> {
    let parsed = Url::parse(value).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if parsed.scheme() != "https" {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL must use https".to_string(),
        ));
    }
    if parsed.host_str().is_none() {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL is missing host".to_string(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL must not include credentials".to_string(),
        ));
    }
    if !matches!(parsed.path(), "" | "/") || parsed.query().is_some() || parsed.fragment().is_some()
    {
        return Err(DomainError::InvalidData(
            "LAN Sync base URL must be an origin".to_string(),
        ));
    }
    Ok(())
}

fn host_for_pairing_prompt(base_url: &str) -> Result<String, DomainError> {
    let parsed =
        Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    parsed
        .host_str()
        .map(str::to_string)
        .ok_or_else(|| DomainError::InvalidData("LAN Sync base URL is missing host".to_string()))
}

fn validate_pairing_session(
    session: &LanPairingSession,
    token: &str,
    now_ms: u64,
) -> Result<(), DomainError> {
    if token != session.token {
        return Err(DomainError::AuthenticationError(
            "Invalid pairing token".to_string(),
        ));
    }
    if now_ms > session.expires_at_ms {
        return Err(DomainError::AuthenticationError(
            "Pairing expired".to_string(),
        ));
    }
    Ok(())
}

fn default_lan_permissions() -> Permissions {
    Permissions {
        read: true,
        write: false,
        mirror_delete: true,
    }
}

fn decode_device_pubkey_b64url(value: &str) -> Result<Vec<u8>, DomainError> {
    let public_key = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if public_key.len() != 32 {
        return Err(DomainError::InvalidData(
            "LAN Sync device public key must be 32 bytes".to_string(),
        ));
    }

    Ok(public_key)
}

fn device_pubkey_b64url(seed: &str) -> Result<String, DomainError> {
    ttsync_core::crypto::device_pubkey_b64url(seed)
        .map_err(|error| DomainError::InvalidData(error.to_string()))
}

fn parse_device_id(device_id: &str) -> Result<DeviceId, DomainError> {
    DeviceId::new(device_id.to_string())
        .map_err(|error| DomainError::InvalidData(error.to_string()))
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::sync::{mpsc, oneshot};

    use crate::application::services::data_change_reconciler::DataChangeReconciler;
    use crate::application::services::sync_job_coordinator::SyncJobExecutor;
    use crate::domain::models::lan_sync::LanSyncIdentity;
    use crate::domain::models::sync::{
        LocalAppliedChangeSummary, SyncExecutionFailure, SyncExecutionKind, SyncExecutionReport,
        SyncJob, SyncJobSummary,
    };

    struct MemorySettingsRepository {
        manual_default_mode: SyncMode,
    }

    #[async_trait]
    impl LanSyncSettingsRepository for MemorySettingsRepository {
        async fn load_or_create_server_settings(&self) -> Result<LanServerSettings, DomainError> {
            Ok(LanServerSettings {
                port: 51_234,
                auto_start: false,
            })
        }

        async fn load_or_create_sync_preferences(&self) -> Result<SyncPreferences, DomainError> {
            Ok(SyncPreferences {
                manual_default_mode: self.manual_default_mode,
            })
        }

        async fn save_sync_preferences(
            &self,
            _preferences: &SyncPreferences,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MemoryPeerRepository {
        identity: LanSyncIdentity,
        paired_devices: Mutex<Vec<LanSyncPairedDevice>>,
    }

    #[async_trait]
    impl LanPeerRepository for MemoryPeerRepository {
        async fn load_or_create_identity(&self) -> Result<LanSyncIdentity, DomainError> {
            Ok(self.identity.clone())
        }

        async fn load_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError> {
            Ok(self.paired_devices.lock().await.clone())
        }

        async fn upsert_paired_device(
            &self,
            device: LanSyncPairedDevice,
        ) -> Result<(), DomainError> {
            let mut devices = self.paired_devices.lock().await;
            devices.retain(|existing| existing.grant.device_id != device.grant.device_id);
            devices.push(device);
            Ok(())
        }

        async fn remove_paired_device(&self, device_id: &DeviceId) -> Result<(), DomainError> {
            self.paired_devices
                .lock()
                .await
                .retain(|device| &device.grant.device_id != device_id);
            Ok(())
        }
    }

    struct StaticApproval {
        accept: bool,
        requests: Mutex<Vec<LanPairingApprovalRequest>>,
    }

    #[async_trait]
    impl PairingApproval for StaticApproval {
        async fn request(&self, request: LanPairingApprovalRequest) -> Result<bool, DomainError> {
            self.requests.lock().await.push(request);
            Ok(self.accept)
        }

        async fn confirm(&self, _request_id: &str, _accept: bool) -> Result<(), DomainError> {
            Ok(())
        }

        async fn cancel_all(&self) {}
    }

    struct NoopEvents;

    impl LanSyncEventPublisher for NoopEvents {
        fn publish_progress(&self, _payload: LanSyncSyncProgressEvent) {}
        fn publish_completed(&self, _payload: LanSyncSyncCompletedEvent) {}
        fn publish_error(&self, _payload: LanSyncSyncErrorEvent) {}
    }

    struct RecordingEvents {
        completed: mpsc::UnboundedSender<LanSyncSyncCompletedEvent>,
    }

    impl LanSyncEventPublisher for RecordingEvents {
        fn publish_progress(&self, _payload: LanSyncSyncProgressEvent) {}

        fn publish_completed(&self, payload: LanSyncSyncCompletedEvent) {
            self.completed.send(payload).expect("record completion");
        }

        fn publish_error(&self, _payload: LanSyncSyncErrorEvent) {}
    }

    struct RecordingExecutor {
        jobs: mpsc::UnboundedSender<SyncJob>,
    }

    #[async_trait]
    impl SyncJobExecutor for RecordingExecutor {
        async fn execute(&self, job: SyncJob) -> Result<SyncExecutionReport, SyncExecutionFailure> {
            self.jobs.send(job).expect("record sync job");
            Ok(SyncExecutionReport::completed(
                SyncJobSummary::new(0, 0, 0),
                LocalAppliedChangeSummary::default(),
            ))
        }
    }

    struct BlockingExecutor {
        started: mpsc::UnboundedSender<()>,
        release: Mutex<Option<oneshot::Receiver<()>>>,
    }

    #[async_trait]
    impl SyncJobExecutor for BlockingExecutor {
        async fn execute(
            &self,
            _job: SyncJob,
        ) -> Result<SyncExecutionReport, SyncExecutionFailure> {
            self.started.send(()).expect("record started job");
            let release = self.release.lock().await.take().expect("release receiver");
            let _ = release.await;
            Ok(SyncExecutionReport::completed(
                SyncJobSummary::new(1, 2, 3),
                LocalAppliedChangeSummary::default(),
            ))
        }
    }

    struct NoopReconciler;

    #[async_trait]
    impl DataChangeReconciler for NoopReconciler {
        async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MemoryServerControl;

    #[async_trait]
    impl LanServerControl for MemoryServerControl {
        async fn start(&self, _port: u16) -> Result<LanServerInfo, DomainError> {
            Ok(LanServerInfo {
                port: 51_234,
                spki_sha256: "server-spki".to_string(),
            })
        }

        async fn stop(&self) -> Result<(), DomainError> {
            Ok(())
        }

        async fn running_info(&self) -> Option<LanServerInfo> {
            Some(LanServerInfo {
                port: 51_234,
                spki_sha256: "server-spki".to_string(),
            })
        }
    }

    struct NoopAddressDiscovery;

    #[async_trait]
    impl LanAddressDiscovery for NoopAddressDiscovery {
        fn list_available_addresses(&self, _port: u16) -> Result<Vec<String>, DomainError> {
            Ok(Vec::new())
        }

        fn default_advertise_address(
            &self,
            _port: u16,
            _available_addresses: &[String],
        ) -> Option<String> {
            None
        }

        async fn routed_advertise_address(
            &self,
            _peer_base_url: &str,
            _local_port: u16,
        ) -> Result<String, DomainError> {
            Err(DomainError::InternalError("not used".to_string()))
        }
    }

    struct NoopPairingClient;

    #[async_trait]
    impl LanPairingClient for NoopPairingClient {
        async fn complete_pairing(
            &self,
            _base_url: &str,
            _spki_sha256: &str,
            _token: &str,
            _request: &LanPairCompleteRequest,
        ) -> Result<LanPairCompleteResponse, DomainError> {
            Err(DomainError::InternalError("not used".to_string()))
        }
    }

    struct ReplacingApproval {
        state: Arc<LanSyncRuntimeState>,
    }

    #[async_trait]
    impl PairingApproval for ReplacingApproval {
        async fn request(&self, _request: LanPairingApprovalRequest) -> Result<bool, DomainError> {
            self.state
                .set_pairing_session(LanPairingSession {
                    token: "new-token".to_string(),
                    expires_at_ms: now_ms() + 60_000,
                })
                .await;
            Ok(true)
        }

        async fn confirm(&self, _request_id: &str, _accept: bool) -> Result<(), DomainError> {
            Ok(())
        }

        async fn cancel_all(&self) {}
    }

    fn test_device_id(value: &str) -> DeviceId {
        DeviceId::new(value.to_string()).expect("valid device id")
    }

    fn test_identity(device_id: DeviceId, device_name: &str) -> LanSyncIdentity {
        LanSyncIdentity {
            device_id,
            device_name: device_name.to_string(),
            ed25519_seed: ttsync_core::crypto::random_base64url(32),
        }
    }

    fn peer_request(device_id: DeviceId, device_name: &str) -> LanPairCompleteRequest {
        let seed = ttsync_core::crypto::random_base64url(32);
        LanPairCompleteRequest {
            device_id,
            device_name: device_name.to_string(),
            device_pubkey: device_pubkey_b64url(&seed).expect("peer public key"),
            client_base_url: "https://192.168.1.23:51000".to_string(),
            client_spki_sha256: "peer-spki".to_string(),
        }
    }

    fn inbound_service(
        state: Arc<LanSyncRuntimeState>,
        peer_repository: Arc<MemoryPeerRepository>,
        approval: Arc<StaticApproval>,
        jobs: mpsc::UnboundedSender<SyncJob>,
        mode: SyncMode,
    ) -> LanInboundService {
        let settings_repository = Arc::new(MemorySettingsRepository {
            manual_default_mode: mode,
        });
        let coordinator = Arc::new(SyncJobCoordinator::new(
            Arc::new(RecordingExecutor { jobs }),
            Arc::new(NoopReconciler),
        ));

        LanInboundService::new(
            state,
            settings_repository,
            peer_repository,
            coordinator,
            Arc::new(NoopEvents),
            approval,
        )
    }

    #[tokio::test]
    async fn inbound_pairing_accepts_peer_and_clears_session() {
        let state = Arc::new(LanSyncRuntimeState::new());
        state
            .set_pairing_session(LanPairingSession {
                token: "pair-token".to_string(),
                expires_at_ms: now_ms() + 60_000,
            })
            .await;

        let identity = test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        );
        let peer_repository = Arc::new(MemoryPeerRepository {
            identity: identity.clone(),
            paired_devices: Mutex::new(Vec::new()),
        });
        let approval = Arc::new(StaticApproval {
            accept: true,
            requests: Mutex::new(Vec::new()),
        });
        let (jobs, _job_rx) = mpsc::unbounded_channel();
        let inbound = inbound_service(
            state.clone(),
            peer_repository.clone(),
            approval.clone(),
            jobs,
            SyncMode::Incremental,
        );
        let peer_id = test_device_id("22222222-2222-4222-8222-222222222222");

        let response = inbound
            .complete_pairing(
                "pair-token".to_string(),
                peer_request(peer_id.clone(), "peer"),
            )
            .await
            .expect("complete pairing");

        assert_eq!(response.server_device_id, identity.device_id);
        assert_eq!(response.server_device_name, "server");
        assert_eq!(response.granted_permissions, default_lan_permissions());
        assert!(state.get_pairing_session().await.is_none());

        let devices = peer_repository.load_paired_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].grant.device_id, peer_id);
        assert_eq!(devices[0].grant.device_name, "peer");
        assert_eq!(devices[0].base_url, "https://192.168.1.23:51000");
        assert_eq!(devices[0].spki_sha256, "peer-spki");
        assert_eq!(devices[0].grant.permissions, default_lan_permissions());

        let approval_requests = approval.requests.lock().await;
        assert_eq!(approval_requests.len(), 1);
        assert_eq!(approval_requests[0].peer_device_name, "peer");
        assert_eq!(approval_requests[0].peer_ip, "192.168.1.23");
    }

    #[tokio::test]
    async fn inbound_pairing_rejection_does_not_store_peer() {
        let state = Arc::new(LanSyncRuntimeState::new());
        state
            .set_pairing_session(LanPairingSession {
                token: "pair-token".to_string(),
                expires_at_ms: now_ms() + 60_000,
            })
            .await;

        let peer_repository = Arc::new(MemoryPeerRepository {
            identity: test_identity(
                test_device_id("11111111-1111-4111-8111-111111111111"),
                "server",
            ),
            paired_devices: Mutex::new(Vec::new()),
        });
        let approval = Arc::new(StaticApproval {
            accept: false,
            requests: Mutex::new(Vec::new()),
        });
        let (jobs, _job_rx) = mpsc::unbounded_channel();
        let inbound = inbound_service(
            state.clone(),
            peer_repository.clone(),
            approval,
            jobs,
            SyncMode::Incremental,
        );

        let error = inbound
            .complete_pairing(
                "pair-token".to_string(),
                peer_request(
                    test_device_id("22222222-2222-4222-8222-222222222222"),
                    "peer",
                ),
            )
            .await
            .expect_err("pairing should be rejected");

        assert!(matches!(
            error,
            DomainError::AuthenticationError(message) if message == PAIRING_REJECTED_MESSAGE
        ));
        assert!(
            peer_repository
                .load_paired_devices()
                .await
                .unwrap()
                .is_empty()
        );
        assert!(state.get_pairing_session().await.is_some());
    }

    #[tokio::test]
    async fn accepted_stale_pairing_request_does_not_clear_new_session() {
        let state = Arc::new(LanSyncRuntimeState::new());
        state
            .set_pairing_session(LanPairingSession {
                token: "old-token".to_string(),
                expires_at_ms: now_ms() + 60_000,
            })
            .await;

        let peer_repository = Arc::new(MemoryPeerRepository {
            identity: test_identity(
                test_device_id("11111111-1111-4111-8111-111111111111"),
                "server",
            ),
            paired_devices: Mutex::new(Vec::new()),
        });
        let (jobs, _job_rx) = mpsc::unbounded_channel();
        let inbound = LanInboundService::new(
            state.clone(),
            Arc::new(MemorySettingsRepository {
                manual_default_mode: SyncMode::Incremental,
            }),
            peer_repository.clone(),
            Arc::new(SyncJobCoordinator::new(
                Arc::new(RecordingExecutor { jobs }),
                Arc::new(NoopReconciler),
            )),
            Arc::new(NoopEvents),
            Arc::new(ReplacingApproval {
                state: state.clone(),
            }),
        );

        let error = inbound
            .complete_pairing(
                "old-token".to_string(),
                peer_request(
                    test_device_id("22222222-2222-4222-8222-222222222222"),
                    "peer",
                ),
            )
            .await
            .expect_err("stale pairing should fail");

        assert!(matches!(
            error,
            DomainError::AuthenticationError(message) if message == "Invalid pairing token"
        ));
        assert_eq!(
            state.get_pairing_session().await.unwrap().token,
            "new-token"
        );
        assert!(
            peer_repository
                .load_paired_devices()
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn inbound_pull_request_starts_remote_request_job() {
        let state = Arc::new(LanSyncRuntimeState::new());
        let peer_repository = Arc::new(MemoryPeerRepository {
            identity: test_identity(
                test_device_id("11111111-1111-4111-8111-111111111111"),
                "server",
            ),
            paired_devices: Mutex::new(Vec::new()),
        });
        let approval = Arc::new(StaticApproval {
            accept: true,
            requests: Mutex::new(Vec::new()),
        });
        let (jobs, mut job_rx) = mpsc::unbounded_channel();
        let inbound = inbound_service(state, peer_repository, approval, jobs, SyncMode::Mirror);
        let peer_id = test_device_id("22222222-2222-4222-8222-222222222222");

        inbound
            .accept_pull_request(peer_id.clone(), SyncOperationOptions::default())
            .await
            .expect("accept pull request");

        let job = tokio::time::timeout(std::time::Duration::from_secs(1), job_rx.recv())
            .await
            .expect("job should execute")
            .expect("job should be recorded");
        assert_eq!(job.execution, SyncExecutionKind::Pull);
        assert_eq!(job.intent, SyncIntent::PullToLocal);
        assert_eq!(
            job.origin,
            SyncOrigin::RemoteRequest {
                peer_id: peer_id.clone()
            }
        );
        match job.endpoint {
            SyncEndpointRef::LanPeer { device_id } => assert_eq!(device_id, peer_id),
            other => panic!("unexpected endpoint: {other:?}"),
        }
        match job.policy {
            ResolvedSyncPolicy::Transfer { mode, .. } => assert_eq!(mode, SyncMode::Mirror),
            other => panic!("unexpected policy: {other:?}"),
        }
    }

    #[tokio::test]
    async fn stop_server_does_not_abort_accepted_inbound_job() {
        let state = Arc::new(LanSyncRuntimeState::new());
        let settings_repository = Arc::new(MemorySettingsRepository {
            manual_default_mode: SyncMode::Incremental,
        });
        let peer_repository = Arc::new(MemoryPeerRepository {
            identity: test_identity(
                test_device_id("11111111-1111-4111-8111-111111111111"),
                "server",
            ),
            paired_devices: Mutex::new(Vec::new()),
        });
        let approval = Arc::new(StaticApproval {
            accept: true,
            requests: Mutex::new(Vec::new()),
        });
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (completed_tx, mut completed_rx) = mpsc::unbounded_channel();
        let coordinator = Arc::new(SyncJobCoordinator::new(
            Arc::new(BlockingExecutor {
                started: started_tx,
                release: Mutex::new(Some(release_rx)),
            }),
            Arc::new(NoopReconciler),
        ));
        let inbound = LanInboundService::new(
            state.clone(),
            settings_repository.clone(),
            peer_repository.clone(),
            coordinator.clone(),
            Arc::new(RecordingEvents {
                completed: completed_tx,
            }),
            approval.clone(),
        );
        let service = LanSyncService::new(
            state,
            settings_repository,
            peer_repository,
            Arc::new(MemoryServerControl),
            Arc::new(NoopAddressDiscovery),
            Arc::new(NoopPairingClient),
            approval,
            coordinator,
        );

        inbound
            .accept_pull_request(
                test_device_id("22222222-2222-4222-8222-222222222222"),
                SyncOperationOptions::default(),
            )
            .await
            .expect("accept pull request");
        tokio::time::timeout(std::time::Duration::from_secs(1), started_rx.recv())
            .await
            .expect("job should start")
            .expect("started job");

        service.stop_server().await.expect("stop server");
        release_tx.send(()).expect("release job");

        let completed =
            tokio::time::timeout(std::time::Duration::from_secs(1), completed_rx.recv())
                .await
                .expect("job should complete after stop")
                .expect("completion event");
        assert_eq!(completed.files_total, 1);
        assert_eq!(completed.bytes_total, 2);
        assert_eq!(completed.files_deleted, 3);
    }

    #[test]
    fn pair_uri_round_trips_required_fields() {
        let uri = build_pair_uri("https://127.0.0.1:50000", "token", 1234, "spki")
            .expect("build pair uri");

        let parsed = parse_pair_uri(&uri).expect("parse pair uri");

        assert_eq!(parsed.base_url, "https://127.0.0.1:50000");
        assert_eq!(parsed.token, "token");
        assert_eq!(parsed.expires_at_ms, 1234);
        assert_eq!(parsed.spki_sha256, "spki");
    }

    #[test]
    fn pair_uri_rejects_http_base_url() {
        assert!(matches!(
            build_pair_uri("http://127.0.0.1:50000", "token", 1234, "spki"),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn base_url_rejects_non_origin_values() {
        for value in [
            "https://127.0.0.1:50000/path",
            "https://127.0.0.1:50000?x=1",
            "https://127.0.0.1:50000#fragment",
            "https://user@127.0.0.1:50000",
        ] {
            assert!(matches!(
                validate_https_base_url(value),
                Err(DomainError::InvalidData(_))
            ));
        }
    }

    #[test]
    fn pair_uri_rejects_legacy_version() {
        assert!(matches!(
            parse_pair_uri(
                "tauritavern://lan-sync/pair?v=1&addr=http%3A%2F%2F127.0.0.1%3A50000&pair_code=x"
            ),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn device_pubkey_requires_32_bytes() {
        let encoded = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert_eq!(
            decode_device_pubkey_b64url(&encoded).unwrap(),
            vec![7u8; 32]
        );

        let short = URL_SAFE_NO_PAD.encode([7u8; 31]);
        assert!(matches!(
            decode_device_pubkey_b64url(&short),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn default_permissions_allow_read_and_mirror_delete_only() {
        let permissions = default_lan_permissions();
        assert!(permissions.read);
        assert!(permissions.mirror_delete);
        assert!(!permissions.write);
    }
}
