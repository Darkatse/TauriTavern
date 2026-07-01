use async_trait::async_trait;
#[cfg(test)]
use reqwest::Response;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::session::{SessionOpenResponse, SessionToken};
use ttsync_contract::status::StatusResponse;

use crate::infrastructure::sync::http_client::{
    SyncHttpClient, bearer_auth_value, ensure_dataset_scope_v1, ensure_success,
};
use crate::infrastructure::sync::lan::server::LAN_PULL_REQUEST_SELECTION_FEATURE_V1;
use crate::infrastructure::sync::lan::store::LanPeerStore;
use tt_contracts::sync::SyncOperationOptions;
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::{LanPairCompleteRequest, LanPairCompleteResponse};
use tt_ports::lan_sync::LanPairingClient;

#[derive(Clone)]
pub struct LanSyncClient {
    inner: SyncHttpClient,
}

impl LanSyncClient {
    pub fn new(base_url: String, spki_sha256: String) -> Result<Self, DomainError> {
        Ok(Self {
            inner: SyncHttpClient::new(base_url, spki_sha256)?,
        })
    }

    pub async fn pair_complete(
        &self,
        token: &str,
        request: &LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError> {
        let mut url = self.inner.endpoint_url("/v2/lan/pair/complete")?;
        url.query_pairs_mut().append_pair("token", token);

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

    pub async fn status(&self) -> Result<StatusResponse, DomainError> {
        self.inner.status().await
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
        selection: ttsync_contract::dataset::DatasetSelection,
        target_manifest: ttsync_contract::manifest::ManifestV2,
    ) -> Result<ttsync_contract::plan::SyncPlan, DomainError> {
        self.inner
            .pull_plan(session_token, mode, selection, target_manifest)
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

pub async fn complete_pairing(
    base_url: &str,
    spki_sha256: &str,
    token: &str,
    request: &LanPairCompleteRequest,
) -> Result<LanPairCompleteResponse, DomainError> {
    LanSyncClient::new(base_url.to_string(), spki_sha256.to_string())?
        .pair_complete(token, request)
        .await
}

pub struct HttpLanPairingClient;

#[async_trait]
impl LanPairingClient for HttpLanPairingClient {
    async fn complete_pairing(
        &self,
        base_url: &str,
        spki_sha256: &str,
        token: &str,
        request: &LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError> {
        complete_pairing(base_url, spki_sha256, token, request).await
    }
}

pub async fn request_peer_pull(
    store: LanPeerStore,
    device_id: &DeviceId,
    options: SyncOperationOptions,
) -> Result<(), DomainError> {
    let peer = store.get_paired_device(device_id).await?;
    let identity = store.load_or_create_identity().await?;

    let api = LanSyncClient::new(peer.base_url.clone(), peer.spki_sha256.clone())?;
    let status = api.status().await?;
    ensure_dataset_scope_v1(&status, "LAN Sync peer")?;
    if !status
        .features
        .iter()
        .any(|feature| feature == LAN_PULL_REQUEST_SELECTION_FEATURE_V1)
    {
        return Err(DomainError::InvalidData(
            "LAN Sync peer does not support scoped pull requests".to_string(),
        ));
    }
    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;
    store
        .update_paired_device(device_id, |peer| {
            peer.grant.permissions = session.granted_permissions;
        })
        .await?;

    api.notify_pull_request(&session.session_token, &options)
        .await
}
