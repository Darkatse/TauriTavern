use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::Client;
use url::Url;

use ttsync_contract::canonical::CanonicalRequest;
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse};
use ttsync_contract::path::SyncPath;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::plan::{CommitResponse, PlanId, PullPlanRequest, PushPlanRequest, SyncPlan};
use ttsync_contract::session::{SessionOpenRequest, SessionOpenResponse, SessionToken, HEADER_DEVICE_ID, HEADER_NONCE, HEADER_SIGNATURE, HEADER_TIMESTAMP_MS};
use ttsync_contract::sync::SyncMode;

use crate::domain::errors::DomainError;
use crate::infrastructure::http_client::apply_default_user_agent;
use crate::infrastructure::tt_sync::crypto::{random_base64url, sha256_base64url};
use crate::infrastructure::tt_sync::identity::sign_ed25519_b64url;
use crate::infrastructure::tt_sync::tls_pin::build_spki_pinned_tls_config;

#[derive(Clone)]
pub struct TtSyncV2Api {
    base_url: String,
    http: Client,
}

impl TtSyncV2Api {
    pub fn new(base_url: String, spki_sha256: String) -> Result<Self, DomainError> {
        let parsed = Url::parse(&base_url)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        if parsed.scheme() != "https" {
            return Err(DomainError::InvalidData(format!(
                "TT-Sync base_url must be https: {}",
                base_url
            )));
        }

        let tls = build_spki_pinned_tls_config(&spki_sha256)?;

        let builder = apply_default_user_agent(Client::builder())
            .use_preconfigured_tls(tls);

        let http = builder
            .build()
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        Ok(Self { base_url, http })
    }

    pub async fn pair_complete(
        &self,
        token: &str,
        request: &PairCompleteRequest,
    ) -> Result<PairCompleteResponse, DomainError> {
        let url = pair_complete_url(&self.base_url, token)?;

        let response = self
            .http
            .post(url)
            .json(request)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "TT-Sync pairing failed").await?;
        response
            .json::<PairCompleteResponse>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub async fn open_session(
        &self,
        device_id: &DeviceId,
        ed25519_seed_b64url: &str,
    ) -> Result<SessionOpenResponse, DomainError> {
        let url = session_open_url(&self.base_url)?;

        let now_ms = now_ms();
        let nonce = random_base64url(12);

        let request = SessionOpenRequest {
            device_id: device_id.clone(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        let body_hash = sha256_base64url(&body);

        let canonical = CanonicalRequest::new(
            device_id.clone(),
            now_ms,
            nonce.clone(),
            "POST".to_owned(),
            "/v2/session/open".to_owned(),
            body_hash,
        )
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        let signature = sign_ed25519_b64url(ed25519_seed_b64url, &canonical.to_bytes())?;

        let response = self
            .http
            .post(url)
            .header(HEADER_DEVICE_ID, device_id.as_str())
            .header(HEADER_TIMESTAMP_MS, now_ms.to_string())
            .header(HEADER_NONCE, nonce)
            .header(HEADER_SIGNATURE, signature)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "TT-Sync session open failed").await?;
        response
            .json::<SessionOpenResponse>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub async fn pull_plan(
        &self,
        session_token: &SessionToken,
        mode: SyncMode,
        target_manifest: ManifestV2,
    ) -> Result<SyncPlan, DomainError> {
        let url = pull_plan_url(&self.base_url)?;

        let request = PullPlanRequest {
            mode,
            target_manifest,
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = self
            .http
            .post(url)
            .header(reqwest::header::AUTHORIZATION, bearer(session_token))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "TT-Sync pull plan failed").await?;
        response
            .json::<SyncPlan>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub async fn push_plan(
        &self,
        session_token: &SessionToken,
        mode: SyncMode,
        source_manifest: ManifestV2,
    ) -> Result<SyncPlan, DomainError> {
        let url = push_plan_url(&self.base_url)?;

        let request = PushPlanRequest {
            mode,
            source_manifest,
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = self
            .http
            .post(url)
            .header(reqwest::header::AUTHORIZATION, bearer(session_token))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "TT-Sync push plan failed").await?;
        response
            .json::<SyncPlan>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }

    pub async fn download_file(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
        path: &SyncPath,
    ) -> Result<reqwest::Response, DomainError> {
        let path_b64 = URL_SAFE_NO_PAD.encode(path.as_str().as_bytes());
        let url = file_download_url(&self.base_url, plan_id, &path_b64)?;

        let response = self
            .http
            .get(url)
            .header(reqwest::header::AUTHORIZATION, bearer(session_token))
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        ensure_success(response, "TT-Sync file download failed").await
    }

    pub async fn upload_file(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
        path: &SyncPath,
        body: reqwest::Body,
    ) -> Result<(), DomainError> {
        let path_b64 = URL_SAFE_NO_PAD.encode(path.as_str().as_bytes());
        let url = file_download_url(&self.base_url, plan_id, &path_b64)?;

        let response = self
            .http
            .put(url)
            .header(reqwest::header::AUTHORIZATION, bearer(session_token))
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "TT-Sync file upload failed").await?;
        response
            .bytes()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        Ok(())
    }

    pub async fn commit(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
    ) -> Result<CommitResponse, DomainError> {
        let url = commit_url(&self.base_url, plan_id)?;

        let response = self
            .http
            .post(url)
            .header(reqwest::header::AUTHORIZATION, bearer(session_token))
            .send()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let response = ensure_success(response, "TT-Sync commit failed").await?;
        response
            .json::<CommitResponse>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))
    }
}

fn pair_complete_url(base_url: &str, token: &str) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    url.set_path("/v2/pair/complete");
    url.set_query(None);
    url.query_pairs_mut().append_pair("token", token);
    Ok(url)
}

fn session_open_url(base_url: &str) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    url.set_path("/v2/session/open");
    url.set_query(None);
    Ok(url)
}

fn pull_plan_url(base_url: &str) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    url.set_path("/v2/sync/pull-plan");
    url.set_query(None);
    Ok(url)
}

fn push_plan_url(base_url: &str) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    url.set_path("/v2/sync/push-plan");
    url.set_query(None);
    Ok(url)
}

fn file_download_url(base_url: &str, plan_id: &PlanId, path_b64: &str) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    url.set_path(&format!(
        "/v2/plans/{}/files/{}",
        plan_id.0, path_b64
    ));
    url.set_query(None);
    Ok(url)
}

fn commit_url(base_url: &str, plan_id: &PlanId) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    url.set_path(&format!("/v2/plans/{}/commit", plan_id.0));
    url.set_query(None);
    Ok(url)
}

fn bearer(session_token: &SessionToken) -> String {
    format!("Bearer {}", session_token.as_str())
}

async fn ensure_success(
    response: reqwest::Response,
    context: &str,
) -> Result<reqwest::Response, DomainError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let message = format!("{} ({}): {}", context, status, body);

    match status.as_u16() {
        400 => Err(DomainError::InvalidData(message)),
        401 => Err(DomainError::AuthenticationError(message)),
        404 => Err(DomainError::NotFound(message)),
        _ => Err(DomainError::InternalError(message)),
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
