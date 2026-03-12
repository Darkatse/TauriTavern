use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use reqwest::Client;
use serde_json::json;
use tokio::sync::oneshot;
use tokio_util::io::ReaderStream;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncDiffPlan, LanSyncManifest, LanSyncPairRequest, LanSyncPairResponse, LanSyncPairedDevice,
};
use crate::infrastructure::http_client::build_http_client;
use crate::infrastructure::lan_sync::crypto::{derive_pair_secret, verify_request_signature};
use crate::infrastructure::lan_sync::manifest::{diff_manifests, scan_manifest};
use crate::infrastructure::lan_sync::paths::{resolve_relative_path, validate_relative_path};
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;

pub struct LanSyncServerHandle {
    pub addr: SocketAddr,
    shutdown_tx: oneshot::Sender<()>,
    _task: tokio::task::JoinHandle<()>,
}

impl LanSyncServerHandle {
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }
}

pub async fn spawn_lan_sync_server(
    addr: SocketAddr,
    runtime: Arc<LanSyncRuntime>,
) -> std::io::Result<LanSyncServerHandle> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let app = Router::new()
        .route("/v1/status", get(handle_status))
        .route("/v1/pair", post(handle_pair))
        .route("/v1/sync/pull", post(handle_sync_pull))
        .route("/v1/sync/plan", post(handle_sync_plan))
        .route("/v1/sync/file/*path", get(handle_sync_file))
        .with_state(runtime);

    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        })
        .await
        {
            tracing::error!("LAN Sync server failed: {}", error);
        }
    });

    Ok(LanSyncServerHandle {
        addr,
        shutdown_tx,
        _task: task,
    })
}

async fn handle_status() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "ok": true })))
}

fn require_auth_headers(headers: &HeaderMap) -> Result<(&str, &str), (StatusCode, String)> {
    let device_id = headers
        .get("X-TT-Device-Id")
        .and_then(|value| value.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "Missing device id".to_string()))?;

    let signature = headers
        .get("X-TT-Signature")
        .and_then(|value| value.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "Missing signature".to_string()))?;

    Ok((device_id, signature))
}

async fn handle_pair(
    State(runtime): State<Arc<LanSyncRuntime>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LanSyncPairRequest>,
) -> impl IntoResponse {
    match handle_pair_inner(runtime, peer_addr, headers, payload).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn handle_sync_plan(
    State(runtime): State<Arc<LanSyncRuntime>>,
    headers: HeaderMap,
    Json(target_manifest): Json<LanSyncManifest>,
) -> impl IntoResponse {
    match handle_sync_plan_inner(runtime, headers, target_manifest).await {
        Ok(plan) => (StatusCode::OK, Json(plan)).into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn handle_sync_plan_inner(
    runtime: Arc<LanSyncRuntime>,
    headers: HeaderMap,
    target_manifest: LanSyncManifest,
) -> Result<LanSyncDiffPlan, (StatusCode, String)> {
    let (device_id, signature) = require_auth_headers(&headers)?;

    for entry in &target_manifest.entries {
        validate_relative_path(&entry.relative_path).map_err(map_domain_error)?;
    }

    let paired_device =
        runtime
            .get_paired_device(device_id)
            .await
            .map_err(|error| match error {
                DomainError::NotFound(_) => {
                    (StatusCode::UNAUTHORIZED, "Unknown device".to_string())
                }
                other => map_domain_error(other),
            })?;

    let body = serde_json::to_vec(&target_manifest)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    if !verify_request_signature(
        paired_device.pair_secret.as_bytes(),
        "POST",
        "/v1/sync/plan",
        &body,
        signature,
    ) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid signature".to_string()));
    }

    let source_manifest = scan_manifest(runtime.sync_root.clone())
        .await
        .map_err(map_domain_error)?;

    Ok(diff_manifests(&source_manifest, &target_manifest))
}

async fn handle_sync_file(
    State(runtime): State<Arc<LanSyncRuntime>>,
    headers: HeaderMap,
    Path(path): Path<String>,
) -> impl IntoResponse {
    match handle_sync_file_inner(runtime, headers, path).await {
        Ok((headers, body)) => (StatusCode::OK, headers, body).into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn handle_sync_file_inner(
    runtime: Arc<LanSyncRuntime>,
    headers: HeaderMap,
    path: String,
) -> Result<(HeaderMap, Body), (StatusCode, String)> {
    validate_relative_path(&path).map_err(map_domain_error)?;

    let (device_id, signature) = require_auth_headers(&headers)?;

    let paired_device =
        runtime
            .get_paired_device(device_id)
            .await
            .map_err(|error| match error {
                DomainError::NotFound(_) => {
                    (StatusCode::UNAUTHORIZED, "Unknown device".to_string())
                }
                other => map_domain_error(other),
            })?;

    let canonical_path = format!("/v1/sync/file/{}", path);
    if !verify_request_signature(
        paired_device.pair_secret.as_bytes(),
        "GET",
        &canonical_path,
        &[],
        signature,
    ) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid signature".to_string()));
    }

    let full_path = resolve_relative_path(&runtime.sync_root, &path).map_err(map_domain_error)?;

    let metadata = tokio::fs::metadata(&full_path)
        .await
        .map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;

    let file = tokio::fs::File::open(&full_path)
        .await
        .map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response_headers.insert(
        axum::http::header::CONTENT_LENGTH,
        HeaderValue::from_str(&metadata.len().to_string())
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
    );

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((response_headers, body))
}

async fn handle_pair_inner(
    runtime: Arc<LanSyncRuntime>,
    peer_addr: SocketAddr,
    headers: HeaderMap,
    payload: LanSyncPairRequest,
) -> Result<LanSyncPairResponse, (StatusCode, String)> {
    let session = runtime
        .get_pairing_session()
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Pairing not enabled".to_string()))?;

    if now_ms() > session.expires_at_ms {
        return Err((StatusCode::UNAUTHORIZED, "Pairing expired".to_string()));
    }

    let signature = headers
        .get("X-TT-Signature")
        .and_then(|value| value.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "Missing signature".to_string()))?;

    let body = serde_json::to_vec(&payload)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    if !verify_request_signature(
        session.pair_code.as_bytes(),
        "POST",
        "/v1/pair",
        &body,
        signature,
    ) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid signature".to_string()));
    }

    let identity = runtime
        .store
        .load_or_create_identity()
        .await
        .map_err(map_domain_error)?;

    let accepted = runtime
        .request_pairing_decision(
            payload.target_device_id.clone(),
            payload.target_device_name.clone(),
            peer_addr.ip().to_string(),
        )
        .await
        .map_err(map_domain_error)?;

    if !accepted {
        return Err((StatusCode::FORBIDDEN, "Pairing rejected".to_string()));
    }

    let pair_secret = derive_pair_secret(
        &session.pair_code,
        &identity.device_id,
        &payload.target_device_id,
    );

    let target_addr = SocketAddr::from((peer_addr.ip(), payload.target_port));

    runtime
        .upsert_paired_device(LanSyncPairedDevice {
            device_id: payload.target_device_id,
            device_name: payload.target_device_name,
            pair_secret,
            last_known_address: Some(format!("http://{}", target_addr)),
            paired_at_ms: now_ms(),
            last_sync_ms: None,
        })
        .await
        .map_err(map_domain_error)?;

    Ok(LanSyncPairResponse {
        source_device_id: identity.device_id,
        source_device_name: identity.device_name,
    })
}

async fn handle_sync_pull(
    State(runtime): State<Arc<LanSyncRuntime>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    match handle_sync_pull_inner(runtime, headers, body).await {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response(),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn handle_sync_pull_inner(
    runtime: Arc<LanSyncRuntime>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(), (StatusCode, String)> {
    let (device_id, signature) = require_auth_headers(&headers)?;

    let paired_device =
        runtime
            .get_paired_device(device_id)
            .await
            .map_err(|error| match error {
                DomainError::NotFound(_) => {
                    (StatusCode::UNAUTHORIZED, "Unknown device".to_string())
                }
                other => map_domain_error(other),
            })?;

    if !verify_request_signature(
        paired_device.pair_secret.as_bytes(),
        "POST",
        "/v1/sync/pull",
        &body,
        signature,
    ) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid signature".to_string()));
    }

    let permit = runtime
        .try_acquire_sync_permit()
        .map_err(map_domain_error)?;

    let device_id = device_id.to_string();
    let runtime_for_task = runtime.clone();
    tokio::spawn(async move {
        let _permit = permit;

        let http_client =
            build_http_client(Client::builder()).expect("Failed to build LAN sync HTTP client");

        match crate::infrastructure::lan_sync::client::merge_sync_from_device(
            runtime_for_task.clone(),
            &http_client,
            &device_id,
        )
        .await
        {
            Ok(completed) => {
                if let Err(error) = runtime_for_task.emit_sync_completed(completed) {
                    tracing::error!("Failed to emit LAN sync completion: {}", error);
                }
            }
            Err(error) => {
                if let Err(error) = runtime_for_task.emit_sync_error(
                    crate::domain::models::lan_sync::LanSyncSyncErrorEvent {
                        message: error.to_string(),
                    },
                ) {
                    tracing::error!("Failed to emit LAN sync error: {}", error);
                }
            }
        }
    });

    Ok(())
}

fn map_domain_error(error: DomainError) -> (StatusCode, String) {
    match error {
        DomainError::NotFound(message) => (StatusCode::NOT_FOUND, message),
        DomainError::InvalidData(message) => (StatusCode::BAD_REQUEST, message),
        DomainError::AuthenticationError(message) => (StatusCode::UNAUTHORIZED, message),
        DomainError::InternalError(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        DomainError::RateLimited { message } => (StatusCode::TOO_MANY_REQUESTS, message),
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
