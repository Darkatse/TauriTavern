use reqwest::{Client, Response};
use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DATASET_SCOPE_FEATURE_V1};
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse, PairUri};
use ttsync_contract::peer::DeviceId;
use ttsync_contract::session::{SessionOpenResponse, SessionToken};
use ttsync_contract::status::StatusResponse;
use ttsync_core::error::SyncError;
use ttsync_http::client::{
    SyncClient, SyncClientHttpVersion, SyncClientOptions,
    bearer_auth_value as shared_bearer_auth_value, ensure_success as shared_ensure_success,
};
use url::Url;

use crate::application::services::tt_sync_service::TtPairingClient;
use crate::domain::errors::DomainError;
use crate::infrastructure::http_client::APP_USER_AGENT;

#[derive(Clone)]
pub struct SyncHttpClient {
    inner: SyncClient,
}

impl SyncHttpClient {
    pub fn new(base_url: String, spki_sha256: String) -> Result<Self, DomainError> {
        let inner = new_sync_client(base_url, spki_sha256)?;
        Ok(Self { inner })
    }

    pub(crate) fn http(&self) -> &Client {
        self.inner.http()
    }

    pub(crate) fn endpoint_url(&self, path: &str) -> Result<Url, DomainError> {
        self.inner.endpoint_url(path).map_err(sync_error_to_domain)
    }

    pub async fn pair_complete(
        &self,
        token: &str,
        request: &PairCompleteRequest,
    ) -> Result<PairCompleteResponse, DomainError> {
        self.inner
            .pair_complete(token, request)
            .await
            .map_err(sync_error_to_domain)
    }

    pub async fn status(&self) -> Result<StatusResponse, DomainError> {
        self.inner.status().await.map_err(sync_error_to_domain)
    }

    pub async fn open_session(
        &self,
        device_id: &DeviceId,
        ed25519_seed_b64url: &str,
    ) -> Result<SessionOpenResponse, DomainError> {
        self.inner
            .open_session(device_id, ed25519_seed_b64url)
            .await
            .map_err(sync_error_to_domain)
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
            .map_err(sync_error_to_domain)
    }

    #[cfg(test)]
    pub async fn download_file(
        &self,
        session_token: &SessionToken,
        plan_id: &ttsync_contract::plan::PlanId,
        path: &ttsync_contract::path::SyncPath,
    ) -> Result<Response, DomainError> {
        self.inner
            .download_file(session_token, plan_id, path)
            .await
            .map_err(sync_error_to_domain)
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
            .map_err(sync_error_to_domain)
    }
}

pub(crate) fn new_sync_client(
    base_url: String,
    spki_sha256: String,
) -> Result<SyncClient, DomainError> {
    SyncClient::with_options(
        base_url,
        SyncClientOptions {
            spki_sha256: Some(spki_sha256),
            user_agent: Some(APP_USER_AGENT.to_string()),
            http_version: SyncClientHttpVersion::Http1Only,
        },
    )
    .map_err(sync_error_to_domain)
}

pub(crate) fn bearer_auth_value(session_token: &SessionToken) -> String {
    shared_bearer_auth_value(session_token)
}

pub(crate) async fn ensure_success(
    response: Response,
    context: &str,
) -> Result<Response, DomainError> {
    shared_ensure_success(response, context)
        .await
        .map_err(sync_error_to_domain)
}

pub(crate) fn ensure_dataset_scope_v1(
    status: &StatusResponse,
    peer_label: &str,
) -> Result<(), DomainError> {
    if !status
        .features
        .iter()
        .any(|feature| feature == DATASET_SCOPE_FEATURE_V1)
    {
        return Err(DomainError::InvalidData(format!(
            "{peer_label} does not support DatasetPolicy"
        )));
    }

    let Some(version) = status.dataset_policy_version else {
        return Err(DomainError::InvalidData(format!(
            "{peer_label} did not report dataset policy version"
        )));
    };

    if version != DATASET_POLICY_VERSION {
        return Err(DomainError::InvalidData(format!(
            "Unsupported {peer_label} dataset policy version: {version}"
        )));
    }

    Ok(())
}

pub(crate) fn sync_error_to_domain(error: SyncError) -> DomainError {
    match error {
        SyncError::NotFound(message) => DomainError::NotFound(message),
        SyncError::InvalidData(message) => DomainError::InvalidData(message),
        SyncError::Unauthorized(message) => DomainError::AuthenticationError(message),
        SyncError::Io(message) | SyncError::Internal(message) => {
            DomainError::InternalError(message)
        }
    }
}

pub(crate) fn domain_error_to_sync(error: DomainError) -> SyncError {
    match error {
        DomainError::NotFound(message) => SyncError::NotFound(message),
        DomainError::InvalidData(message) => SyncError::InvalidData(message),
        DomainError::AuthenticationError(message) => SyncError::Unauthorized(message),
        DomainError::Cancelled(message) => SyncError::Internal(message),
        DomainError::InternalError(message) => SyncError::Internal(message),
        DomainError::RateLimited { message } => SyncError::Internal(message),
        DomainError::Transient(message) => SyncError::Io(message),
        DomainError::UpstreamFailure(failure) => SyncError::Internal(failure.to_string()),
        DomainError::WorkspacePathIsDirectory { path } => {
            SyncError::InvalidData(format!("Workspace path is a directory: {path}"))
        }
        DomainError::WorkspaceWriteConflict { kind, .. } => {
            SyncError::InvalidData(format!("Workspace write conflict: {kind}"))
        }
    }
}

pub struct HttpTtPairingClient;

#[async_trait::async_trait]
impl TtPairingClient for HttpTtPairingClient {
    async fn complete_pairing(
        &self,
        pair: &PairUri,
        request: &PairCompleteRequest,
    ) -> Result<PairCompleteResponse, DomainError> {
        SyncHttpClient::new(pair.url.clone(), pair.spki_sha256.clone())?
            .pair_complete(&pair.token, request)
            .await
    }
}

#[cfg(test)]
mod tests {
    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DATASET_SCOPE_FEATURE_V1};
    use ttsync_contract::status::StatusResponse;
    use ttsync_core::error::SyncError;

    use super::{ensure_dataset_scope_v1, sync_error_to_domain};
    use crate::domain::errors::DomainError;

    fn status(features: Vec<String>, version: Option<u32>) -> StatusResponse {
        StatusResponse {
            ok: true,
            protocol: "v2".to_owned(),
            server: "tt-sync".to_owned(),
            features,
            dataset_policy_version: version,
            supported_dataset_ids: vec![],
            supported_profile_ids: vec![],
            default_dataset_ids: vec![],
            device_id: None,
            device_name: None,
            spki_sha256: None,
        }
    }

    #[test]
    fn sync_error_maps_to_domain_error() {
        assert!(matches!(
            sync_error_to_domain(SyncError::Unauthorized("nope".to_owned())),
            DomainError::AuthenticationError(_)
        ));
        assert!(matches!(
            sync_error_to_domain(SyncError::InvalidData("bad".to_owned())),
            DomainError::InvalidData(_)
        ));
    }

    #[test]
    fn status_requires_dataset_scope_feature() {
        let status = status(vec![], Some(DATASET_POLICY_VERSION));
        assert!(matches!(
            ensure_dataset_scope_v1(&status, "TT-Sync server"),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn status_accepts_matching_dataset_policy_version() {
        let status = status(
            vec![DATASET_SCOPE_FEATURE_V1.to_owned()],
            Some(DATASET_POLICY_VERSION),
        );
        ensure_dataset_scope_v1(&status, "TT-Sync server")
            .expect("dataset scope feature should be accepted");
    }
}
