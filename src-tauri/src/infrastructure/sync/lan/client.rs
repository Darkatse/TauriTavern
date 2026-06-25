use reqwest::Response;
use ttsync_contract::dataset::DatasetSelection;
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::session::{SessionOpenResponse, SessionToken};
use ttsync_contract::status::StatusResponse;
use ttsync_contract::sync::SyncMode;

use crate::domain::errors::DomainError;
use crate::domain::models::sync::SyncOperationOptions;
use crate::infrastructure::sync::http_client::{SyncHttpClient, bearer_auth_value, ensure_success};
use crate::infrastructure::sync::lan::pairing::{LanPairCompleteRequest, LanPairCompleteResponse};

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

    pub async fn pull_plan(
        &self,
        session_token: &SessionToken,
        mode: SyncMode,
        selection: DatasetSelection,
        target_manifest: ManifestV2,
    ) -> Result<SyncPlan, DomainError> {
        self.inner
            .pull_plan(session_token, mode, selection, target_manifest)
            .await
    }

    pub async fn download_file(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
        path: &SyncPath,
    ) -> Result<Response, DomainError> {
        self.inner.download_file(session_token, plan_id, path).await
    }

    pub async fn download_bundle(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
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
