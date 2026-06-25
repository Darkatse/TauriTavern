use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use local_ip_address::{list_afinet_netifas, local_ip};
use qrcode::QrCode;
use tauri::Manager;
use tokio::sync::Mutex;
use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_contract::sync::SyncMode;
use url::Url;

use crate::app::AppState;
use crate::application::services::sync_job_coordinator::SyncJobCoordinator;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncPairedDevice, LanSyncPairedDeviceSummary, LanSyncStatus, LanSyncSyncCompletedEvent,
    LanSyncSyncErrorEvent,
};
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin, resolve_sync_options,
};
use crate::infrastructure::lan_sync::runtime::{LanSyncPairingSession, LanSyncRuntime};
use crate::infrastructure::sync::http_client::sync_error_to_domain;
use crate::infrastructure::sync::lan::client::complete_pairing as complete_lan_pairing;
use crate::infrastructure::sync::lan::notify::LanSyncNotifyPullHandler;
use crate::infrastructure::sync::lan::pairing::{
    LanPairCompleteRequest, decode_device_pubkey_b64url, validate_https_base_url,
};
use crate::infrastructure::sync::lan::server::{LanSyncServerHandle, spawn_lan_sync_server};
use crate::infrastructure::sync::lan::store::LanPeerStore;

pub struct LanSyncService {
    runtime: Arc<LanSyncRuntime>,
    peer_store: LanPeerStore,
    coordinator: Arc<SyncJobCoordinator>,
    server: Mutex<Option<LanSyncServerHandle>>,
}

impl LanSyncService {
    pub fn new(
        runtime: Arc<LanSyncRuntime>,
        peer_store: LanPeerStore,
        coordinator: Arc<SyncJobCoordinator>,
    ) -> Self {
        Self {
            peer_store,
            runtime,
            coordinator,
            server: Mutex::new(None),
        }
    }

    pub async fn get_status(&self) -> Result<LanSyncStatus, DomainError> {
        let settings = self.runtime.store.load_or_create_server_settings().await?;
        let (sync_mode, manual_default_mode, sync_mode_overridden) = self.sync_mode_state().await?;

        let pairing = self.runtime.get_pairing_session().await;
        let now_ms = now_ms();

        let pairing_enabled = pairing
            .as_ref()
            .is_some_and(|session| session.expires_at_ms > now_ms);
        let pairing_expires_at_ms = pairing.as_ref().map(|session| session.expires_at_ms);

        let running_info = self.running_server_info().await;
        let (running, port) = match running_info.as_ref() {
            Some(info) => (true, info.port),
            None => (false, settings.port),
        };

        let available_addresses = list_available_addresses(port)?;
        let address = default_advertise_address(port, &available_addresses);
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
        let (sync_mode, manual_default_mode, sync_mode_overridden) = self.sync_mode_state().await?;

        let pairing = self.runtime.get_pairing_session().await;
        let now_ms = now_ms();
        let pairing_enabled = pairing
            .as_ref()
            .is_some_and(|session| session.expires_at_ms > now_ms);
        let pairing_expires_at_ms = pairing.as_ref().map(|session| session.expires_at_ms);

        let port = {
            let mut server = self.server.lock().await;
            match server.as_ref() {
                Some(handle) => handle.addr.port(),
                None => {
                    let handle = self.spawn_server().await?;
                    let port = handle.addr.port();
                    *server = Some(handle);
                    port
                }
            }
        };

        let available_addresses = list_available_addresses(port)?;
        let address = default_advertise_address(port, &available_addresses);

        Ok(LanSyncStatus {
            running: true,
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

    pub async fn stop_server(&self) -> Result<(), DomainError> {
        let handle = {
            let mut server = self.server.lock().await;
            server.take()
        };
        let Some(handle) = handle else {
            return Ok(());
        };

        handle.shutdown();
        self.runtime.clear_pairing_session().await;
        Ok(())
    }

    pub async fn set_sync_mode(&self, mode: SyncMode, persist: bool) -> Result<(), DomainError> {
        if persist {
            let mut preferences = self.runtime.store.load_or_create_sync_preferences().await?;
            preferences.manual_default_mode = mode;
            self.runtime
                .store
                .save_sync_preferences(&preferences)
                .await?;
            self.runtime.set_sync_mode_override(None).await;
            return Ok(());
        }

        self.runtime.set_sync_mode_override(Some(mode)).await;
        Ok(())
    }

    pub async fn clear_sync_mode_override(&self) {
        self.runtime.set_sync_mode_override(None).await;
    }

    pub async fn effective_sync_mode(&self) -> Result<SyncMode, DomainError> {
        self.runtime.effective_sync_mode().await
    }

    async fn sync_mode_state(&self) -> Result<(SyncMode, SyncMode, bool), DomainError> {
        let preferences = self.runtime.store.load_or_create_sync_preferences().await?;
        let sync_mode_override = self.runtime.get_sync_mode_override().await;
        let manual_default_mode = preferences.manual_default_mode;
        let sync_mode_overridden = sync_mode_override.is_some();
        Ok((
            sync_mode_override.unwrap_or(manual_default_mode),
            manual_default_mode,
            sync_mode_overridden,
        ))
    }

    async fn running_server_info(&self) -> Option<LanSyncServerInfo> {
        let server = self.server.lock().await;
        server.as_ref().map(|handle| LanSyncServerInfo {
            port: handle.addr.port(),
            spki_sha256: handle.spki_sha256.clone(),
        })
    }

    async fn ensure_server_running(&self) -> Result<(), DomainError> {
        if self.server.lock().await.is_none() {
            return Err(DomainError::InvalidData(
                "LAN Sync server is not running".to_string(),
            ));
        }

        Ok(())
    }

    async fn spawn_server(&self) -> Result<LanSyncServerHandle, DomainError> {
        let settings = self.runtime.store.load_or_create_server_settings().await?;
        let addr = std::net::SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, settings.port));
        let notify_pull = Arc::new(LanSyncNotifyPullHandler::new(
            self.runtime.clone(),
            self.coordinator.clone(),
        ));
        spawn_lan_sync_server(
            addr,
            self.runtime.sync_root.clone(),
            self.peer_store.clone(),
            self.runtime.clone(),
            notify_pull,
        )
        .await
    }

    pub async fn enable_pairing(
        &self,
        advertise_address: Option<String>,
    ) -> Result<LanSyncPairingInfo, DomainError> {
        let server_info = self.running_server_info().await.ok_or_else(|| {
            DomainError::InvalidData("LAN sync server is not running".to_string())
        })?;

        let address = match advertise_address {
            Some(value) => {
                validate_https_base_url(&value)?;
                value
            }
            None => {
                let available_addresses = list_available_addresses(server_info.port)?;
                default_advertise_address(server_info.port, &available_addresses).ok_or_else(
                    || DomainError::InvalidData("No available LAN sync addresses".to_string()),
                )?
            }
        };

        let expires_at_ms = now_ms() + 5 * 60 * 1000;
        let token = ttsync_core::crypto::random_base64url(16);

        self.runtime
            .set_pairing_session(LanSyncPairingSession {
                token: token.clone(),
                expires_at_ms,
            })
            .await;

        let pair_uri = build_pair_uri(&address, &token, expires_at_ms, &server_info.spki_sha256)?;
        let qr_svg = generate_qr_svg(&pair_uri)?;

        Ok(LanSyncPairingInfo {
            address,
            pair_uri,
            qr_svg,
            expires_at_ms,
        })
    }

    pub async fn get_pairing_info(
        &self,
        advertise_address: &str,
    ) -> Result<LanSyncPairingInfo, DomainError> {
        let server_info = self.running_server_info().await.ok_or_else(|| {
            DomainError::InvalidData("LAN sync server is not running".to_string())
        })?;
        validate_https_base_url(advertise_address)?;

        let session = self.runtime.get_pairing_session().await.ok_or_else(|| {
            DomainError::InvalidData("LAN sync pairing is not enabled".to_string())
        })?;

        if now_ms() > session.expires_at_ms {
            return Err(DomainError::InvalidData(
                "LAN sync pairing expired".to_string(),
            ));
        }

        let pair_uri = build_pair_uri(
            advertise_address,
            &session.token,
            session.expires_at_ms,
            &server_info.spki_sha256,
        )?;
        let qr_svg = generate_qr_svg(&pair_uri)?;

        Ok(LanSyncPairingInfo {
            address: advertise_address.to_string(),
            pair_uri,
            qr_svg,
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

        let server_info = self.running_server_info().await.ok_or_else(|| {
            DomainError::InvalidData("LAN sync server must be running before pairing".to_string())
        })?;
        let local_base_url =
            routed_lan_advertise_address(&parsed.base_url, server_info.port).await?;

        let identity = self.peer_store.load_or_create_identity().await?;
        let device_pubkey = ttsync_core::crypto::device_pubkey_b64url(&identity.ed25519_seed)
            .map_err(sync_error_to_domain)?;
        let request = LanPairCompleteRequest {
            device_id: identity.device_id.clone(),
            device_name: identity.device_name.clone(),
            device_pubkey,
            client_base_url: local_base_url,
            client_spki_sha256: server_info.spki_sha256,
        };

        let response = complete_lan_pairing(
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

        let public_key = decode_device_pubkey_b64url(&response.server_device_pubkey)?;
        let paired_device = LanSyncPairedDevice {
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

        self.peer_store
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
        Ok(self
            .peer_store
            .load_paired_devices()
            .await?
            .into_iter()
            .map(LanSyncPairedDeviceSummary::from)
            .collect())
    }

    pub async fn remove_paired_device(&self, device_id: &str) -> Result<(), DomainError> {
        let device_id = parse_device_id(device_id)?;
        self.peer_store.remove_paired_device(&device_id).await?;
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
        let started = match self.coordinator.try_start(request) {
            Ok(started) => started,
            Err(report) => {
                if let Some(message) = report.failure_message() {
                    self.runtime.emit_sync_error(LanSyncSyncErrorEvent {
                        message: message.to_string(),
                    })?;
                }
                return Ok(report);
            }
        };

        let executed = started.execute().await;
        let report = if executed.outcome().is_some() {
            match self
                .runtime
                .app_handle()
                .state::<Arc<AppState>>()
                .refresh_after_external_data_change("lan_sync")
                .await
            {
                Ok(()) => executed.finish(),
                Err(error) => {
                    let message = format!(
                        "LAN sync completed but failed to refresh runtime caches: {}",
                        error
                    );
                    let report = executed.finish_with_error(error);
                    self.runtime
                        .emit_sync_error(LanSyncSyncErrorEvent { message })?;
                    return Ok(report);
                }
            }
        } else {
            executed.finish()
        };

        if let Some(summary) = report.completed_summary() {
            self.runtime
                .emit_sync_completed(LanSyncSyncCompletedEvent {
                    files_total: summary.files_total,
                    bytes_total: summary.bytes_total,
                    files_deleted: summary.files_deleted,
                })?;
        } else if let Some(message) = report.failure_message() {
            self.runtime.emit_sync_error(LanSyncSyncErrorEvent {
                message: message.to_string(),
            })?;
        }

        Ok(report)
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

    pub async fn push_to_device_for_automation(
        &self,
        device_id: &str,
        options: Option<SyncOperationOptions>,
    ) -> Result<SyncJobReport, DomainError> {
        self.ensure_server_running().await?;
        let device_id = parse_device_id(device_id)?;
        let options = resolve_sync_options(options)?;
        self.run_request_remote_pull(device_id, SyncOrigin::Scheduled, options)
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
    pub qr_svg: String,
    pub expires_at_ms: u64,
}

struct LanSyncServerInfo {
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
                    Some(format!("https://{}:{}", ip, port))
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
        std::net::IpAddr::V4(v4) => Some(format!("https://{}:{}", v4, port)),
        std::net::IpAddr::V6(_) => None,
    });

    route_ip
        .filter(|addr| available_addresses.contains(addr))
        .or_else(|| available_addresses.first().cloned())
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

async fn routed_lan_advertise_address(
    peer_base_url: &str,
    local_port: u16,
) -> Result<String, DomainError> {
    validate_https_base_url(peer_base_url)?;
    let peer_url =
        Url::parse(peer_base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let peer_host = peer_url
        .host_str()
        .ok_or_else(|| DomainError::InvalidData("LAN Sync peer URL is missing host".to_string()))?;
    let peer_port = peer_url
        .port_or_known_default()
        .ok_or_else(|| DomainError::InvalidData("LAN Sync peer URL is missing port".to_string()))?;

    let remote_addr = tokio::net::lookup_host((peer_host, peer_port))
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .find(|addr| addr.is_ipv4())
        .ok_or_else(|| {
            DomainError::InvalidData("No IPv4 LAN Sync peer address resolved".to_string())
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
            "No routable IPv4 LAN Sync address".to_string(),
        )),
    }
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

fn parse_device_id(device_id: &str) -> Result<DeviceId, DomainError> {
    DeviceId::new(device_id.to_string())
        .map_err(|error| DomainError::InvalidData(error.to_string()))
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
    fn pair_uri_rejects_legacy_version() {
        assert!(matches!(
            parse_pair_uri(
                "tauritavern://lan-sync/pair?v=1&addr=http%3A%2F%2F127.0.0.1%3A50000&pair_code=x"
            ),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[tokio::test]
    async fn routed_lan_advertise_address_uses_peer_route() {
        let address = routed_lan_advertise_address("https://127.0.0.1:50000", 56000)
            .await
            .expect("routed address");

        assert_eq!(address, "https://127.0.0.1:56000");
    }
}
