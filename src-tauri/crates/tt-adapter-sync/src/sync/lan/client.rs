use std::time::Duration;

use async_trait::async_trait;
#[cfg(test)]
use reqwest::Response;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::session::{SessionOpenResponse, SessionToken};
use ttsync_contract::status::StatusResponse;
use ttsync_contract::sync::OverwritePolicy;

use super::LanStatusResponse;
use super::discovery::LocalLanAddressDiscovery;
use crate::sync::http_client::{
    SyncHttpClient, bearer_auth_value, ensure_dataset_scope_v1, ensure_success,
};
use crate::sync::lan::peer_discovery::LanPeerDiscovery;
use crate::sync::lan::server::{
    LAN_PULL_REQUEST_OVERWRITE_POLICY_FEATURE_V1, LAN_PULL_REQUEST_SELECTION_FEATURE_V1,
};
use crate::sync::lan::store::LanPeerStore;
use tt_contracts::lan_discovery::LanDiscoveryAnnouncement;
use tt_contracts::sync::SyncOperationOptions;
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::{
    LanPairCompleteRequest, LanPairCompleteResponse, LanSyncPairedDevice,
};
use tt_ports::lan_discovery::LanDeviceDiscovery;
use tt_ports::lan_sync::{LanAddressDiscovery, LanPairingClient};

#[derive(Clone)]
pub struct LanSyncClient {
    inner: SyncHttpClient,
}

impl LanSyncClient {
    pub(crate) fn into_sync_client(self) -> ttsync_http::client::SyncClient {
        self.inner.into_sync_client()
    }

    pub fn new(
        base_url: String,
        spki_sha256: String,
        product_user_agent: &str,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            inner: SyncHttpClient::new(base_url, spki_sha256, product_user_agent)?,
        })
    }

    pub async fn pair_complete(
        &self,
        request: &LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError> {
        let url = self.inner.endpoint_url("/v2/lan/pair/complete")?;

        let response = self
            .inner
            .http()
            .post(url)
            .json(request)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "LAN Sync pairing failed").await?;
        response
            .json::<LanPairCompleteResponse>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub(crate) async fn status(&self) -> Result<LanStatusResponse, DomainError> {
        let response = self
            .inner
            .http()
            .get(self.inner.endpoint_url("/v2/status")?)
            .send()
            .await
            .map_err(|error| DomainError::Transient(error.to_string()))?;
        ensure_success(response, "Cannot read LAN device status")
            .await?
            .json()
            .await
            .map_err(|error| DomainError::InvalidData(error.to_string()))
    }

    pub async fn open_session(
        &self,
        device_id: &DeviceId,
        ed25519_seed_b64url: &str,
    ) -> Result<SessionOpenResponse, DomainError> {
        self.inner
            .open_session(device_id, ed25519_seed_b64url)
            .await
    }

    #[cfg(test)]
    pub async fn pull_plan(
        &self,
        session_token: &SessionToken,
        mode: ttsync_contract::sync::SyncMode,
        overwrite_policy: ttsync_contract::sync::OverwritePolicy,
        selection: ttsync_contract::dataset::DatasetSelection,
        target_manifest: ttsync_contract::manifest::ManifestV2,
    ) -> Result<ttsync_contract::plan::SyncPlan, DomainError> {
        self.inner
            .pull_plan(
                session_token,
                mode,
                overwrite_policy,
                selection,
                target_manifest,
            )
            .await
    }

    #[cfg(test)]
    pub async fn download_file(
        &self,
        session_token: &SessionToken,
        plan_id: &ttsync_contract::plan::PlanId,
        path: &ttsync_contract::path::SyncPath,
    ) -> Result<Response, DomainError> {
        self.inner.download_file(session_token, plan_id, path).await
    }

    #[cfg(test)]
    pub async fn download_bundle(
        &self,
        session_token: &SessionToken,
        plan_id: &ttsync_contract::plan::PlanId,
        accept_zstd: bool,
    ) -> Result<Response, DomainError> {
        self.inner
            .download_bundle(session_token, plan_id, accept_zstd)
            .await
    }

    pub async fn notify_pull_request(
        &self,
        session_token: &SessionToken,
        options: &SyncOperationOptions,
    ) -> Result<(), DomainError> {
        let url = self.inner.endpoint_url("/v2/lan/pull-request")?;

        let response = self
            .inner
            .http()
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .json(options)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "LAN Sync pull request failed").await?;
        response
            .bytes()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        Ok(())
    }
}

pub struct HttpLanPairingClient {
    product_user_agent: String,
}

impl HttpLanPairingClient {
    pub fn new(product_user_agent: impl Into<String>) -> Self {
        let product_user_agent = product_user_agent.into();
        assert!(
            !product_user_agent.trim().is_empty(),
            "sync product user agent must not be empty"
        );
        Self { product_user_agent }
    }
}

#[async_trait]
impl LanPairingClient for HttpLanPairingClient {
    async fn probe_device(
        &self,
        base_url: &str,
        spki_sha256: Option<&str>,
    ) -> Result<LanDiscoveryAnnouncement, DomainError> {
        let status = if let Some(pin) = spki_sha256 {
            probe_endpoint(base_url, pin, None, &self.product_user_agent)
                .await?
                .1
        } else {
            // Only public discovery metadata crosses this untrusted connection. All pairing
            // and authenticated operations use the separate, pinned client below.
            let http = reqwest::Client::builder()
                .tls_danger_accept_invalid_certs(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .https_only(true)
                .timeout(Duration::from_secs(6))
                .user_agent(&self.product_user_agent)
                .build()
                .map_err(|error| DomainError::InternalError(error.to_string()))?;
            let response = http
                .get(format!("{base_url}/v2/status"))
                .send()
                .await
                .map_err(|error| {
                    DomainError::Transient(format!("Cannot connect to {base_url}: {error}"))
                })?;
            ensure_success(response, "Cannot read LAN device status")
                .await?
                .json::<LanStatusResponse>()
                .await
                .map_err(|error| DomainError::InvalidData(error.to_string()))?
        };
        if status.status.protocol != "lan-v2" {
            return Err(DomainError::InvalidData(
                "Device does not support LAN Sync v2".to_string(),
            ));
        }
        let announcement = LanDiscoveryAnnouncement {
            device_id: status.status.device_id.ok_or_else(|| {
                DomainError::InvalidData("Missing LAN device identity".to_string())
            })?,
            device_name: status.status.device_name.unwrap_or_default(),
            platform: status.platform,
            port: url::Url::parse(base_url)
                .ok()
                .and_then(|url| url.port_or_known_default())
                .unwrap_or(0),
            spki_sha256: status.status.spki_sha256.unwrap_or_default(),
        };
        super::peer_discovery::validate_announcement(&announcement)?;
        Ok(announcement)
    }

    async fn complete_pairing(
        &self,
        base_urls: &[String],
        spki_sha256: &str,
        expected_device_id: Option<&DeviceId>,
        local_device: &LanDiscoveryAnnouncement,
        device_pubkey: &str,
    ) -> Result<(LanPairCompleteResponse, String), DomainError> {
        let (api, base_url) = tokio::time::timeout(Duration::from_secs(6), async {
            let mut last_error = DomainError::NotFound("No LAN Sync address available".to_string());
            for url in base_urls {
                match probe_endpoint(
                    url,
                    spki_sha256,
                    expected_device_id,
                    &self.product_user_agent,
                )
                .await
                {
                    Ok((api, _)) => return Ok((api, url.clone())),
                    Err(error) => last_error = error,
                }
            }
            Err(last_error)
        })
        .await
        .map_err(|_| DomainError::Transient("Timed out locating LAN Sync device".to_string()))??;
        let client_base_url = LocalLanAddressDiscovery
            .routed_advertise_address(&base_url, local_device.port)
            .await?;
        let request = LanPairCompleteRequest {
            device_id: local_device.device_id.clone(),
            device_name: local_device.device_name.clone(),
            device_platform: local_device.platform.clone(),
            device_pubkey: device_pubkey.to_string(),
            client_base_url,
            client_spki_sha256: local_device.spki_sha256.clone(),
        };
        // Once sent, the request may already have been approved. Only address probes are retried.
        let response = api.pair_complete(&request).await?;
        Ok((response, base_url))
    }
}

pub async fn request_peer_pull(
    store: LanPeerStore,
    discovery: &LanPeerDiscovery,
    device_id: &DeviceId,
    options: SyncOperationOptions,
    product_user_agent: &str,
) -> Result<(), DomainError> {
    let peer = store.get_paired_device(device_id).await?;
    let identity = store.load_or_create_identity().await?;

    let (api, status, base_url) = connect_peer(&peer, discovery, product_user_agent).await?;
    ensure_dataset_scope_v1(&status.status, "LAN Sync peer")?;
    if !status
        .status
        .features
        .iter()
        .any(|feature| feature == LAN_PULL_REQUEST_SELECTION_FEATURE_V1)
    {
        return Err(DomainError::InvalidData(
            "LAN Sync peer does not support scoped pull requests".to_string(),
        ));
    }
    ensure_pull_request_overwrite_policy(&status.status, options.overwrite_policy)?;
    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;
    store
        .update_paired_device(device_id, |peer| {
            status.update_peer(peer, &base_url);
            peer.grant.permissions = session.granted_permissions;
        })
        .await?;

    api.notify_pull_request(&session.session_token, &options)
        .await
}

pub(crate) async fn connect_peer(
    peer: &LanSyncPairedDevice,
    discovery: &LanPeerDiscovery,
    product_user_agent: &str,
) -> Result<(LanSyncClient, LanStatusResponse, String), DomainError> {
    // Only locating the peer has a short deadline; file transfers keep their existing lifetime.
    tokio::time::timeout(Duration::from_secs(6), async {
        let mut tried = Vec::new();
        let mut last_error = DomainError::NotFound("No LAN Sync address available".to_string());
        for refresh in [false, true] {
            if (refresh || discovery.candidate_urls(&peer.grant.device_id).is_empty())
                && let Err(error) = discovery.discover_devices().await
            {
                tracing::warn!("LAN discovery unavailable; trying known peer addresses: {error}");
            }
            let mut urls = discovery.candidate_urls(&peer.grant.device_id);
            urls.push(peer.base_url.clone());
            for url in urls {
                if tried.contains(&url) {
                    continue;
                }
                tried.push(url.clone());
                match probe_endpoint(
                    &url,
                    &peer.spki_sha256,
                    Some(&peer.grant.device_id),
                    product_user_agent,
                )
                .await
                {
                    Ok((client, status)) => return Ok((client, status, url)),
                    Err(error) => last_error = error,
                }
            }
        }
        Err(last_error)
    })
    .await
    .map_err(|_| {
        DomainError::Transient(
            "Timed out locating LAN Sync device. Refresh and try again.".to_string(),
        )
    })?
}

async fn probe_endpoint(
    base_url: &str,
    spki_sha256: &str,
    expected_device_id: Option<&DeviceId>,
    product_user_agent: &str,
) -> Result<(LanSyncClient, LanStatusResponse), DomainError> {
    let api = LanSyncClient::new(
        base_url.to_string(),
        spki_sha256.to_string(),
        product_user_agent,
    )?;
    let status = tokio::time::timeout(Duration::from_secs(2), api.status())
        .await
        .map_err(|_| {
            DomainError::Transient(format!("LAN Sync device did not respond at {base_url}"))
        })??;
    if status.status.device_id.is_none()
        || expected_device_id
            .is_some_and(|expected| status.status.device_id.as_ref() != Some(expected))
    {
        return Err(DomainError::AuthenticationError(
            "LAN Sync device identity does not match".to_string(),
        ));
    }
    if status.status.protocol != "lan-v2" {
        return Err(DomainError::InvalidData(
            "Device does not support LAN Sync v2".to_string(),
        ));
    }
    Ok((api, status))
}

fn ensure_pull_request_overwrite_policy(
    status: &StatusResponse,
    overwrite_policy: OverwritePolicy,
) -> Result<(), DomainError> {
    if overwrite_policy == OverwritePolicy::PreferNewer
        && !status
            .features
            .iter()
            .any(|feature| feature == LAN_PULL_REQUEST_OVERWRITE_POLICY_FEATURE_V1)
    {
        return Err(DomainError::InvalidData(
            "LAN Sync peer does not support prefer-newer pull requests".to_string(),
        ));
    }

    Ok(())
}
