use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use local_ip_address::{list_afinet_netifas, local_ip};
use qrcode::QrCode;
use tauri::AppHandle;
use tauri::Manager;
use tokio::sync::Mutex;
use url::Url;

use crate::app::AppState;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncPairRequest, LanSyncPairResponse, LanSyncPairedDevice, LanSyncStatus,
    LanSyncSyncCompletedEvent, LanSyncSyncErrorEvent, LanSyncSyncMode,
};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};
use crate::infrastructure::lan_sync::crypto::{derive_pair_secret, random_base64url, sign_request};
use crate::infrastructure::lan_sync::runtime::{LanSyncPairingSession, LanSyncRuntime};
use crate::infrastructure::lan_sync::server::{LanSyncServerHandle, spawn_lan_sync_server};

pub struct LanSyncService {
    runtime: Arc<LanSyncRuntime>,
    http_clients: Arc<HttpClientPool>,
    server: Mutex<Option<LanSyncServerHandle>>,
}

impl LanSyncService {
    pub fn new(
        app_handle: AppHandle,
        sync_root: PathBuf,
        store_root: PathBuf,
        http_clients: Arc<HttpClientPool>,
    ) -> Self {
        Self {
            runtime: Arc::new(LanSyncRuntime::new(app_handle, sync_root, store_root)),
            http_clients,
            server: Mutex::new(None),
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
            let available_addresses = list_available_addresses(port)?;
            let address = default_advertise_address(port, &available_addresses);
            return Ok(LanSyncStatus {
                running: true,
                address,
                available_addresses,
                port,
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

        let port = handle.addr.port();
        let available_addresses = list_available_addresses(port)?;
        let address = default_advertise_address(port, &available_addresses);

        let status = LanSyncStatus {
            running: true,
            address,
            available_addresses,
            port,
            pairing_enabled,
            pairing_expires_at_ms,
            sync_mode,
            sync_mode_persistent,
            sync_mode_overridden,
        };

        let mut server = self.server.lock().await;
        *server = Some(handle);
        Ok(status)
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
        self.runtime
            .set_pairing_session(LanSyncPairingSession {
                pair_code: pair_code.clone(),
                expires_at_ms,
            })
            .await;

        let pair_uri = build_pair_uri(&address, &pair_code, expires_at_ms)?;
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

        let pair_uri =
            build_pair_uri(advertise_address, &session.pair_code, session.expires_at_ms)?;
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
    ) -> Result<LanSyncPairedDevice, DomainError> {
        let parsed = parse_pair_uri(pair_uri)?;
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

        Ok(paired_device)
    }

    pub async fn confirm_pairing(&self, request_id: &str, accept: bool) -> Result<(), DomainError> {
        self.runtime.confirm_pairing(request_id, accept).await
    }

    pub async fn list_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError> {
        self.runtime.load_paired_devices().await
    }

    pub async fn remove_paired_device(&self, device_id: &str) -> Result<(), DomainError> {
        self.runtime.remove_paired_device(device_id).await
    }

    pub async fn sync_from_device(&self, device_id: &str) -> Result<(), DomainError> {
        let permit = match self.runtime.try_acquire_sync_permit() {
            Ok(permit) => permit,
            Err(error) => {
                self.runtime.emit_sync_error(LanSyncSyncErrorEvent {
                    message: error.to_string(),
                })?;
                return Ok(());
            }
        };

        match self.sync_from_device_inner(device_id).await {
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
    ) -> Result<LanSyncSyncCompletedEvent, DomainError> {
        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        crate::infrastructure::lan_sync::client::merge_sync_from_device(
            self.runtime.clone(),
            &http_client,
            device_id,
        )
        .await
    }

    pub async fn push_to_device(&self, device_id: &str) -> Result<(), DomainError> {
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
}

pub struct LanSyncPairingInfo {
    pub address: String,
    pub pair_uri: String,
    pub qr_svg: String,
    pub expires_at_ms: u64,
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

struct ParsedPairUri {
    address: String,
    pair_code: String,
}

fn parse_pair_uri(pair_uri: &str) -> Result<ParsedPairUri, DomainError> {
    let uri = Url::parse(pair_uri).map_err(|error| DomainError::InvalidData(error.to_string()))?;

    let mut address = None;
    let mut pair_code = None;
    for (key, value) in uri.query_pairs() {
        match key.as_ref() {
            "addr" => address = Some(value.to_string()),
            "pair_code" => pair_code = Some(value.to_string()),
            _ => {}
        }
    }

    Ok(ParsedPairUri {
        address: address.ok_or_else(|| DomainError::InvalidData("Missing addr".to_string()))?,
        pair_code: pair_code
            .ok_or_else(|| DomainError::InvalidData("Missing pair_code".to_string()))?,
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
