use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use local_ip_address::{list_afinet_netifas, local_ip};
use qrcode::QrCode;
use tauri::AppHandle;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use ttsync_contract::peer::{DeviceId, PeerGrant};
use url::Url;

use crate::app::AppState;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncPairRequest, LanSyncPairResponse, LanSyncPairedDevice, LanSyncPairedDeviceSummary,
    LanSyncStatus, LanSyncSyncCompletedEvent, LanSyncSyncErrorEvent, LanSyncSyncMode,
    LanSyncV2PairedDevice,
};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};
use crate::infrastructure::lan_sync::crypto::{derive_pair_secret, random_base64url, sign_request};
use crate::infrastructure::lan_sync::runtime::{LanSyncPairingSession, LanSyncRuntime};
use crate::infrastructure::lan_sync::server::{LanSyncServerHandle, spawn_lan_sync_server};
use crate::infrastructure::lan_sync::v2::client::complete_pairing as complete_v2_pairing;
use crate::infrastructure::lan_sync::v2::notify::{
    LanSyncV2NotifyPullHandler, request_peer_pull as request_v2_peer_pull,
};
use crate::infrastructure::lan_sync::v2::pairing::{
    LanSyncV2PairCompleteRequest, decode_device_pubkey_b64url, validate_https_base_url,
};
use crate::infrastructure::lan_sync::v2::pull::pull_from_device as pull_from_v2_device;
use crate::infrastructure::lan_sync::v2::server::{
    LanSyncV2ServerHandle, spawn_lan_sync_v2_server,
};
use crate::infrastructure::lan_sync::v2::store::LanSyncV2Store;
use crate::infrastructure::sync_v2::{SyncV2OperationOptions, resolve_sync_v2_options};
use crate::infrastructure::tt_sync::v2_api::sync_error_to_domain;

pub struct LanSyncService {
    runtime: Arc<LanSyncRuntime>,
    v2_store: LanSyncV2Store,
    http_clients: Arc<HttpClientPool>,
    server: Mutex<Option<LanSyncServerHandle>>,
    v2_server: Mutex<Option<LanSyncV2ServerHandle>>,
}

impl LanSyncService {
    pub fn new(
        app_handle: AppHandle,
        sync_root: PathBuf,
        store_root: PathBuf,
        http_clients: Arc<HttpClientPool>,
        sync_permit: Arc<Semaphore>,
    ) -> Self {
        let v2_store = LanSyncV2Store::new(store_root.clone());
        Self {
            runtime: Arc::new(LanSyncRuntime::new(
                app_handle,
                sync_root,
                store_root,
                sync_permit,
            )),
            v2_store,
            http_clients,
            server: Mutex::new(None),
            v2_server: Mutex::new(None),
        }
    }

    pub async fn get_status(&self) -> Result<LanSyncStatus, DomainError> {
        let config = self.runtime.store.load_or_create_config().await?;
        let sync_mode_override = self.runtime.get_sync_mode_override().await;
        let sync_mode_persistent = config.sync_mode;
        let sync_mode_overridden = sync_mode_override.is_some();
        let sync_mode = sync_mode_override.unwrap_or(sync_mode_persistent);

        let pairing = self.runtime.get_pairing_session().await;
        let now_ms = now_ms();

        let pairing_enabled = pairing
            .as_ref()
            .is_some_and(|session| session.expires_at_ms > now_ms);
        let pairing_expires_at_ms = pairing.as_ref().map(|session| session.expires_at_ms);

        let running_port = {
            let server = self.server.lock().await;
            server.as_ref().map(|handle| handle.addr.port())
        };
        let (running, port) = match running_port {
            Some(port) => (true, port),
            None => (false, config.port),
        };
        let v2_info = self.running_v2_server_info().await;

        let available_addresses = list_available_addresses(port)?;
        let address = default_advertise_address(port, &available_addresses);
        Ok(LanSyncStatus {
            running,
            address,
            available_addresses,
            port,
            v2_running: v2_info.is_some(),
            v2_port: v2_info.as_ref().map(|info| info.port),
            v2_spki_sha256: v2_info.map(|info| info.spki_sha256),
            pairing_enabled,
            pairing_expires_at_ms,
            sync_mode,
            sync_mode_persistent,
            sync_mode_overridden,
        })
    }

    pub async fn start_server(&self) -> Result<LanSyncStatus, DomainError> {
        let config = self.runtime.store.load_or_create_config().await?;
        let sync_mode_override = self.runtime.get_sync_mode_override().await;
        let sync_mode_persistent = config.sync_mode;
        let sync_mode_overridden = sync_mode_override.is_some();
        let sync_mode = sync_mode_override.unwrap_or(sync_mode_persistent);

        let pairing = self.runtime.get_pairing_session().await;
        let now_ms = now_ms();
        let pairing_enabled = pairing
            .as_ref()
            .is_some_and(|session| session.expires_at_ms > now_ms);
        let pairing_expires_at_ms = pairing.as_ref().map(|session| session.expires_at_ms);

        let running_port = {
            let server = self.server.lock().await;
            server.as_ref().map(|handle| handle.addr.port())
        };
        if let Some(port) = running_port {
            self.ensure_v2_server_started().await?;
            let v2_info = self.running_v2_server_info().await;
            let available_addresses = list_available_addresses(port)?;
            let address = default_advertise_address(port, &available_addresses);
            return Ok(LanSyncStatus {
                running: true,
                address,
                available_addresses,
                port,
                v2_running: v2_info.is_some(),
                v2_port: v2_info.as_ref().map(|info| info.port),
                v2_spki_sha256: v2_info.map(|info| info.spki_sha256),
                pairing_enabled,
                pairing_expires_at_ms,
                sync_mode,
                sync_mode_persistent,
                sync_mode_overridden,
            });
        }

        let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, config.port));
        let handle = spawn_lan_sync_server(addr, self.runtime.clone())
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        let v2_handle = match self.spawn_v2_server().await {
            Ok(handle) => handle,
            Err(error) => {
                handle.shutdown();
                return Err(error);
            }
        };

        let port = handle.addr.port();
        let available_addresses = list_available_addresses(port)?;
        let address = default_advertise_address(port, &available_addresses);
        let v2_info = LanSyncV2ServerInfo {
            port: v2_handle.addr.port(),
            spki_sha256: v2_handle.spki_sha256.clone(),
        };

        let status = LanSyncStatus {
            running: true,
            address,
            available_addresses,
            port,
            v2_running: true,
            v2_port: Some(v2_info.port),
            v2_spki_sha256: Some(v2_info.spki_sha256),
            pairing_enabled,
            pairing_expires_at_ms,
            sync_mode,
            sync_mode_persistent,
            sync_mode_overridden,
        };

        let mut server = self.server.lock().await;
        *server = Some(handle);
        let mut v2_server = self.v2_server.lock().await;
        *v2_server = Some(v2_handle);
        Ok(status)
    }

    pub async fn stop_server(&self) -> Result<(), DomainError> {
        let handle = {
            let mut server = self.server.lock().await;
            server.take()
        };
        let v2_handle = {
            let mut server = self.v2_server.lock().await;
            server.take()
        };
        if let Some(handle) = v2_handle {
            handle.shutdown();
        }

        let Some(handle) = handle else {
            return Ok(());
        };

        handle.shutdown();
        self.runtime.clear_pairing_session().await;
        Ok(())
    }

    pub async fn set_sync_mode(
        &self,
        mode: LanSyncSyncMode,
        persist: bool,
    ) -> Result<(), DomainError> {
        if persist {
            let mut config = self.runtime.store.load_or_create_config().await?;
            config.sync_mode = mode;
            self.runtime.store.save_config(&config).await?;
            self.runtime.set_sync_mode_override(None).await;
            return Ok(());
        }

        self.runtime.set_sync_mode_override(Some(mode)).await;
        Ok(())
    }

    pub async fn clear_sync_mode_override(&self) {
        self.runtime.set_sync_mode_override(None).await;
    }

    pub async fn effective_sync_mode(&self) -> Result<LanSyncSyncMode, DomainError> {
        let config = self.runtime.store.load_or_create_config().await?;
        Ok(self
            .runtime
            .get_sync_mode_override()
            .await
            .unwrap_or(config.sync_mode))
    }

    async fn running_v2_server_info(&self) -> Option<LanSyncV2ServerInfo> {
        let server = self.v2_server.lock().await;
        server.as_ref().map(|handle| LanSyncV2ServerInfo {
            port: handle.addr.port(),
            spki_sha256: handle.spki_sha256.clone(),
        })
    }

    async fn ensure_v2_server_started(&self) -> Result<(), DomainError> {
        if self.running_v2_server_info().await.is_some() {
            return Ok(());
        }

        let handle = self.spawn_v2_server().await?;
        let mut server = self.v2_server.lock().await;
        if server.is_some() {
            handle.shutdown();
        } else {
            *server = Some(handle);
        }

        Ok(())
    }

    async fn spawn_v2_server(&self) -> Result<LanSyncV2ServerHandle, DomainError> {
        let config = self.runtime.store.load_or_create_config().await?;
        let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, config.v2_port));
        let notify_pull = Arc::new(LanSyncV2NotifyPullHandler::new(
            self.runtime.clone(),
            self.v2_store.clone(),
        ));
        spawn_lan_sync_v2_server(
            addr,
            self.runtime.sync_root.clone(),
            self.v2_store.clone(),
            self.runtime.clone(),
            notify_pull,
        )
        .await
    }

    pub async fn enable_pairing(
        &self,
        advertise_address: Option<String>,
    ) -> Result<LanSyncPairingInfo, DomainError> {
        let port = {
            let server = self.server.lock().await;
            server.as_ref().map(|handle| handle.addr.port())
        }
        .ok_or_else(|| DomainError::InvalidData("LAN sync server is not running".to_string()))?;

        let address = match advertise_address {
            Some(value) => value,
            None => {
                let available_addresses = list_available_addresses(port)?;
                default_advertise_address(port, &available_addresses).ok_or_else(|| {
                    DomainError::InvalidData("No available LAN sync addresses".to_string())
                })?
            }
        };

        let expires_at_ms = now_ms() + 5 * 60 * 1000;
        let pair_code = random_base64url(16);
        self.ensure_v2_server_started().await?;
        let v2_info = self.running_v2_server_info().await.ok_or_else(|| {
            DomainError::InternalError("LAN Sync v2 server did not start".to_string())
        })?;

        self.runtime
            .set_pairing_session(LanSyncPairingSession {
                pair_code: pair_code.clone(),
                expires_at_ms,
            })
            .await;

        let pair_uri = build_pair_uri(&address, &pair_code, expires_at_ms)?;
        let qr_svg = generate_qr_svg(&pair_uri)?;
        let v2_address = build_v2_advertise_address(&address, v2_info.port)?;
        let v2_pair_uri =
            build_v2_pair_uri(&v2_address, &pair_code, expires_at_ms, &v2_info.spki_sha256)?;
        let v2_qr_svg = generate_qr_svg(&v2_pair_uri)?;

        Ok(LanSyncPairingInfo {
            address,
            pair_uri,
            qr_svg,
            expires_at_ms,
            v2_address: Some(v2_address),
            v2_pair_uri: Some(v2_pair_uri),
            v2_qr_svg: Some(v2_qr_svg),
        })
    }

    pub async fn get_pairing_info(
        &self,
        advertise_address: &str,
    ) -> Result<LanSyncPairingInfo, DomainError> {
        let server_running = {
            let server = self.server.lock().await;
            server.is_some()
        };
        if !server_running {
            return Err(DomainError::InvalidData(
                "LAN sync server is not running".to_string(),
            ));
        }

        let session = self.runtime.get_pairing_session().await.ok_or_else(|| {
            DomainError::InvalidData("LAN sync pairing is not enabled".to_string())
        })?;

        if now_ms() > session.expires_at_ms {
            return Err(DomainError::InvalidData(
                "LAN sync pairing expired".to_string(),
            ));
        }

        self.ensure_v2_server_started().await?;
        let v2_info = self.running_v2_server_info().await.ok_or_else(|| {
            DomainError::InternalError("LAN Sync v2 server did not start".to_string())
        })?;

        let pair_uri =
            build_pair_uri(advertise_address, &session.pair_code, session.expires_at_ms)?;
        let qr_svg = generate_qr_svg(&pair_uri)?;
        let v2_address = build_v2_advertise_address(advertise_address, v2_info.port)?;
        let v2_pair_uri = build_v2_pair_uri(
            &v2_address,
            &session.pair_code,
            session.expires_at_ms,
            &v2_info.spki_sha256,
        )?;
        let v2_qr_svg = generate_qr_svg(&v2_pair_uri)?;

        Ok(LanSyncPairingInfo {
            address: advertise_address.to_string(),
            pair_uri,
            qr_svg,
            expires_at_ms: session.expires_at_ms,
            v2_address: Some(v2_address),
            v2_pair_uri: Some(v2_pair_uri),
            v2_qr_svg: Some(v2_qr_svg),
        })
    }

    pub async fn request_pairing(
        &self,
        pair_uri: &str,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        match parse_pair_uri(pair_uri)? {
            ParsedPairUri::V1(parsed) => self.request_pairing_v1(parsed).await,
            ParsedPairUri::V2(parsed) => self.request_pairing_v2(parsed).await,
        }
    }

    async fn request_pairing_v1(
        &self,
        parsed: ParsedV1PairUri,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        let identity = self.runtime.store.load_or_create_identity().await?;
        let config = self.runtime.store.load_or_create_config().await?;

        let payload = LanSyncPairRequest {
            target_device_id: identity.device_id.clone(),
            target_device_name: identity.device_name.clone(),
            target_port: config.port,
        };
        let body = serde_json::to_vec(&payload)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        let signature = sign_request(parsed.pair_code.as_bytes(), "POST", "/v1/pair", &body);

        let url = format!("{}/v1/pair", parsed.address.trim_end_matches('/'));
        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client
            .post(url)
            .header("X-TT-Signature", signature)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|error| DomainError::InternalError(error.to_string()))?;
            return Err(DomainError::AuthenticationError(format!(
                "Pairing failed ({}): {}",
                status, body
            )));
        }

        let pair_response = response
            .json::<LanSyncPairResponse>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let pair_secret = derive_pair_secret(
            &parsed.pair_code,
            &pair_response.source_device_id,
            &identity.device_id,
        );

        let paired_device = LanSyncPairedDevice {
            device_id: pair_response.source_device_id,
            device_name: pair_response.source_device_name,
            pair_secret,
            last_known_address: Some(parsed.address),
            paired_at_ms: now_ms(),
            last_sync_ms: None,
        };

        self.runtime
            .upsert_paired_device(paired_device.clone())
            .await?;

        Ok(paired_device.into())
    }

    async fn request_pairing_v2(
        &self,
        parsed: ParsedV2PairUri,
    ) -> Result<LanSyncPairedDeviceSummary, DomainError> {
        if now_ms() > parsed.expires_at_ms {
            return Err(DomainError::InvalidData(
                "LAN Sync v2 pairing expired".to_string(),
            ));
        }

        let v2_info = self.running_v2_server_info().await.ok_or_else(|| {
            DomainError::InvalidData(
                "LAN sync server must be running before LAN Sync v2 pairing".to_string(),
            )
        })?;
        let local_base_url = routed_v2_advertise_address(&parsed.base_url, v2_info.port).await?;

        let identity = self.v2_store.load_or_create_identity().await?;
        let device_pubkey = ttsync_core::crypto::device_pubkey_b64url(&identity.ed25519_seed)
            .map_err(sync_error_to_domain)?;
        let request = LanSyncV2PairCompleteRequest {
            device_id: identity.device_id.clone(),
            device_name: identity.device_name.clone(),
            device_pubkey,
            client_base_url: local_base_url,
            client_spki_sha256: v2_info.spki_sha256,
        };

        let response = complete_v2_pairing(
            &parsed.base_url,
            &parsed.spki_sha256,
            &parsed.token,
            &request,
        )
        .await?;

        if response.server_device_id == identity.device_id {
            return Err(DomainError::InvalidData(
                "Cannot pair LAN Sync v2 device with itself".to_string(),
            ));
        }

        let public_key = decode_device_pubkey_b64url(&response.server_device_pubkey)?;
        let paired_device = LanSyncV2PairedDevice {
            grant: PeerGrant {
                device_id: response.server_device_id,
                device_name: response.server_device_name,
                public_key,
                permissions: response.granted_permissions,
                paired_at_ms: now_ms(),
                last_sync_ms: None,
            },
            base_url: parsed.base_url,
            spki_sha256: parsed.spki_sha256,
        };

        self.v2_store
            .upsert_paired_device(paired_device.clone())
            .await?;

        Ok(paired_device.into())
    }

    pub async fn confirm_pairing(&self, request_id: &str, accept: bool) -> Result<(), DomainError> {
        self.runtime.confirm_pairing(request_id, accept).await
    }

    pub async fn list_paired_devices(
        &self,
    ) -> Result<Vec<LanSyncPairedDeviceSummary>, DomainError> {
        let mut devices = self
            .runtime
            .load_paired_devices()
            .await?
            .into_iter()
            .map(LanSyncPairedDeviceSummary::from)
            .collect::<Vec<_>>();

        devices.extend(
            self.v2_store
                .load_paired_devices()
                .await?
                .into_iter()
                .map(LanSyncPairedDeviceSummary::from),
        );

        Ok(devices)
    }

    pub async fn remove_paired_device(&self, device_id: &str) -> Result<(), DomainError> {
        self.runtime.remove_paired_device(device_id).await?;
        if let Ok(v2_device_id) = DeviceId::new(device_id.to_string()) {
            self.v2_store.remove_paired_device(&v2_device_id).await?;
        }
        Ok(())
    }

    pub async fn sync_from_device(
        &self,
        device_id: &str,
        options: Option<SyncV2OperationOptions>,
    ) -> Result<(), DomainError> {
        let permit = match self.runtime.try_acquire_sync_permit() {
            Ok(permit) => permit,
            Err(error) => {
                self.runtime.emit_sync_error(LanSyncSyncErrorEvent {
                    message: error.to_string(),
                })?;
                return Ok(());
            }
        };

        match self.sync_from_device_inner(device_id, options).await {
            Ok(completed) => {
                let refresh_result = self
                    .runtime
                    .app_handle()
                    .state::<Arc<AppState>>()
                    .refresh_after_external_data_change("lan_sync")
                    .await;
                match refresh_result {
                    Ok(()) => {
                        drop(permit);
                        self.runtime.emit_sync_completed(completed)?;
                    }
                    Err(error) => {
                        drop(permit);
                        self.runtime.emit_sync_error(LanSyncSyncErrorEvent {
                            message: format!(
                                "LAN sync completed but failed to refresh runtime caches: {}",
                                error
                            ),
                        })?;
                    }
                }
            }
            Err(error) => {
                drop(permit);
                self.runtime.emit_sync_error(LanSyncSyncErrorEvent {
                    message: error.to_string(),
                })?;
            }
        }

        Ok(())
    }

    async fn sync_from_device_inner(
        &self,
        device_id: &str,
        options: Option<SyncV2OperationOptions>,
    ) -> Result<LanSyncSyncCompletedEvent, DomainError> {
        if let Some(v2_device_id) = self.resolve_v2_peer_device_id(device_id).await? {
            let options = resolve_sync_v2_options(options)?;
            return pull_from_v2_device(
                self.runtime.clone(),
                self.v2_store.clone(),
                &v2_device_id,
                options,
            )
            .await;
        }

        if options.is_some() {
            return Err(DomainError::InvalidData(
                "LAN Sync v2 pairing is required for scoped sync".to_string(),
            ));
        }

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        crate::infrastructure::lan_sync::client::merge_sync_from_device(
            self.runtime.clone(),
            &http_client,
            device_id,
        )
        .await
    }

    pub async fn push_to_device(
        &self,
        device_id: &str,
        options: Option<SyncV2OperationOptions>,
    ) -> Result<(), DomainError> {
        if let Some(v2_device_id) = self.resolve_v2_peer_device_id(device_id).await? {
            let options = resolve_sync_v2_options(options)?;
            return request_v2_peer_pull(self.v2_store.clone(), &v2_device_id, options).await;
        }

        if options.is_some() {
            return Err(DomainError::InvalidData(
                "LAN Sync v2 pairing is required for scoped sync".to_string(),
            ));
        }

        let peer = self.runtime.get_paired_device(device_id).await?;
        let address = peer.last_known_address.clone().ok_or_else(|| {
            DomainError::InvalidData(format!("Paired device address is missing: {}", device_id))
        })?;

        let identity = self.runtime.store.load_or_create_identity().await?;

        let mut url =
            Url::parse(&address).map_err(|error| DomainError::InvalidData(error.to_string()))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| DomainError::InvalidData("Invalid source address".to_string()))?;
            segments.clear();
            segments.push("v1");
            segments.push("sync");
            segments.push("pull");
        }

        let signature = sign_request(peer.pair_secret.as_bytes(), "POST", "/v1/sync/pull", &[]);

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client
            .post(url)
            .header("X-TT-Device-Id", identity.device_id)
            .header("X-TT-Signature", signature)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|error| DomainError::InternalError(error.to_string()))?;
            return Err(DomainError::AuthenticationError(format!(
                "Push request failed ({}): {}",
                status, body
            )));
        }

        Ok(())
    }

    pub async fn push_to_device_for_automation(
        &self,
        device_id: &str,
        options: Option<SyncV2OperationOptions>,
    ) -> Result<(), DomainError> {
        let server_running = {
            let server = self.server.lock().await;
            server.is_some()
        };
        let v2_server_running = {
            let server = self.v2_server.lock().await;
            server.is_some()
        };
        if !server_running || !v2_server_running {
            return Err(DomainError::InvalidData(
                "LAN Sync server is not running".to_string(),
            ));
        }

        let Some(v2_device_id) = self.resolve_v2_peer_device_id(device_id).await? else {
            return Err(DomainError::InvalidData(
                "LAN auto upload requires a paired LAN Sync v2 device".to_string(),
            ));
        };
        let options = resolve_sync_v2_options(options)?;
        request_v2_peer_pull(self.v2_store.clone(), &v2_device_id, options).await
    }

    async fn resolve_v2_peer_device_id(
        &self,
        device_id: &str,
    ) -> Result<Option<DeviceId>, DomainError> {
        let Ok(device_id) = DeviceId::new(device_id.to_string()) else {
            return Ok(None);
        };

        match self.v2_store.get_paired_device(&device_id).await {
            Ok(_) => Ok(Some(device_id)),
            Err(DomainError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub struct LanSyncPairingInfo {
    pub address: String,
    pub pair_uri: String,
    pub qr_svg: String,
    pub expires_at_ms: u64,
    pub v2_address: Option<String>,
    pub v2_pair_uri: Option<String>,
    pub v2_qr_svg: Option<String>,
}

struct LanSyncV2ServerInfo {
    port: u16,
    spki_sha256: String,
}

fn list_available_addresses(port: u16) -> Result<Vec<String>, DomainError> {
    let ifas =
        list_afinet_netifas().map_err(|error| DomainError::InternalError(error.to_string()))?;

    let mut addresses = ifas
        .into_iter()
        .filter_map(|(_name, ip)| match ip {
            std::net::IpAddr::V4(ip) => {
                if ip.is_loopback() || ip.is_unspecified() {
                    None
                } else {
                    Some(format!("http://{}:{}", ip, port))
                }
            }
            std::net::IpAddr::V6(_) => None,
        })
        .collect::<Vec<_>>();

    addresses.sort();
    addresses.dedup();
    Ok(addresses)
}

fn default_advertise_address(port: u16, available_addresses: &[String]) -> Option<String> {
    let route_ip = local_ip().ok().and_then(|ip| match ip {
        std::net::IpAddr::V4(v4) => Some(format!("http://{}:{}", v4, port)),
        std::net::IpAddr::V6(_) => None,
    });

    route_ip
        .filter(|addr| available_addresses.contains(addr))
        .or_else(|| available_addresses.first().cloned())
}

fn build_pair_uri(
    address: &str,
    pair_code: &str,
    expires_at_ms: u64,
) -> Result<String, DomainError> {
    let mut uri = Url::parse("tauritavern://lan-sync/pair")
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    uri.query_pairs_mut()
        .append_pair("v", "1")
        .append_pair("addr", address)
        .append_pair("pair_code", pair_code)
        .append_pair("exp", &expires_at_ms.to_string());

    Ok(uri.to_string())
}

fn build_v2_pair_uri(
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

fn build_v2_advertise_address(v1_address: &str, v2_port: u16) -> Result<String, DomainError> {
    let mut url =
        Url::parse(v1_address).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if url.host_str().is_none() {
        return Err(DomainError::InvalidData(
            "LAN sync advertise address is missing host".to_string(),
        ));
    }

    url.set_scheme("https")
        .map_err(|_| DomainError::InvalidData("Invalid LAN Sync v2 scheme".to_string()))?;
    url.set_port(Some(v2_port))
        .map_err(|_| DomainError::InvalidData("Invalid LAN Sync v2 port".to_string()))?;
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);

    Ok(url.to_string().trim_end_matches('/').to_string())
}

async fn routed_v2_advertise_address(
    peer_base_url: &str,
    local_port: u16,
) -> Result<String, DomainError> {
    validate_https_base_url(peer_base_url)?;
    let peer_url =
        Url::parse(peer_base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let peer_host = peer_url.host_str().ok_or_else(|| {
        DomainError::InvalidData("LAN Sync v2 peer URL is missing host".to_string())
    })?;
    let peer_port = peer_url.port_or_known_default().ok_or_else(|| {
        DomainError::InvalidData("LAN Sync v2 peer URL is missing port".to_string())
    })?;

    let remote_addr = tokio::net::lookup_host((peer_host, peer_port))
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .find(|addr| addr.is_ipv4())
        .ok_or_else(|| {
            DomainError::InvalidData("No IPv4 LAN Sync v2 peer address resolved".to_string())
        })?;

    let socket = tokio::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    socket
        .connect(remote_addr)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let local_addr = socket
        .local_addr()
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    match local_addr.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => {
            Ok(format!("https://{}:{}", ip, local_port))
        }
        _ => Err(DomainError::InvalidData(
            "No routable IPv4 LAN Sync v2 address".to_string(),
        )),
    }
}

enum ParsedPairUri {
    V1(ParsedV1PairUri),
    V2(ParsedV2PairUri),
}

struct ParsedV1PairUri {
    address: String,
    pair_code: String,
}

struct ParsedV2PairUri {
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
    if version.as_deref() == Some("2") {
        return parse_v2_pair_uri(&uri).map(ParsedPairUri::V2);
    }

    parse_v1_pair_uri(&uri).map(ParsedPairUri::V1)
}

fn parse_v1_pair_uri(uri: &Url) -> Result<ParsedV1PairUri, DomainError> {
    let mut address = None;
    let mut pair_code = None;
    for (key, value) in uri.query_pairs() {
        match key.as_ref() {
            "addr" => address = Some(value.to_string()),
            "pair_code" => pair_code = Some(value.to_string()),
            _ => {}
        }
    }

    Ok(ParsedV1PairUri {
        address: address.ok_or_else(|| DomainError::InvalidData("Missing addr".to_string()))?,
        pair_code: pair_code
            .ok_or_else(|| DomainError::InvalidData("Missing pair_code".to_string()))?,
    })
}

fn parse_v2_pair_uri(uri: &Url) -> Result<ParsedV2PairUri, DomainError> {
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

    Ok(ParsedV2PairUri {
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

fn generate_qr_svg(text: &str) -> Result<String, DomainError> {
    let code = QrCode::new(text.as_bytes())
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .build())
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

    #[test]
    fn v2_pair_uri_round_trips_required_fields() {
        let uri = build_v2_pair_uri("https://127.0.0.1:50000", "token", 1234, "spki")
            .expect("build v2 pair uri");

        let ParsedPairUri::V2(parsed) = parse_pair_uri(&uri).expect("parse v2 pair uri") else {
            panic!("expected v2 pair uri");
        };

        assert_eq!(parsed.base_url, "https://127.0.0.1:50000");
        assert_eq!(parsed.token, "token");
        assert_eq!(parsed.expires_at_ms, 1234);
        assert_eq!(parsed.spki_sha256, "spki");
    }

    #[test]
    fn v2_pair_uri_rejects_http_base_url() {
        assert!(matches!(
            build_v2_pair_uri("http://127.0.0.1:50000", "token", 1234, "spki"),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn v2_advertise_address_uses_selected_host_and_v2_port() {
        let address =
            build_v2_advertise_address("http://192.168.1.20:55000", 56000).expect("v2 address");

        assert_eq!(address, "https://192.168.1.20:56000");
    }

    #[tokio::test]
    async fn routed_v2_advertise_address_uses_peer_route() {
        let address = routed_v2_advertise_address("https://127.0.0.1:50000", 56000)
            .await
            .expect("routed address");

        assert_eq!(address, "https://127.0.0.1:56000");
    }
}
