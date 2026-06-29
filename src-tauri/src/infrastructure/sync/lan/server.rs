use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::json;
use tokio::io::AsyncRead;
use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;
use ttsync_core::ports::{ManifestStore, PeerStore};
use ttsync_core::session::{SessionManager, SessionManagerConfig};
use ttsync_http::server::{ServerState, build_transfer_router, default_status_response};
use ttsync_http::tls::{SelfManagedTls, TlsProvider};

use crate::application::services::lan_sync_service::PAIRING_REJECTED_MESSAGE;
use crate::application::services::lan_sync_service::ports::{
    LanInboundRequestHandler, LanServerInfo,
};
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{LanPairCompleteRequest, LanPairCompleteResponse};
use crate::domain::models::sync::SyncOperationOptions;
use crate::infrastructure::sync::http_client::{domain_error_to_sync, sync_error_to_domain};
use crate::infrastructure::sync::lan::store::LanPeerStore;
use crate::infrastructure::sync_fs;
use crate::infrastructure::sync_transfer;
use crate::infrastructure::tt_sync::fs::scan_manifest_with_policy;

const LAN_HTTPS_FEATURE_V1: &str = "lan_https_v1";
const LAN_SESSION_FEATURE_V1: &str = "lan_session_v1";
pub(crate) const LAN_PULL_REQUEST_SELECTION_FEATURE_V1: &str = "lan_pull_request_selection_v1";

pub struct LanSyncServerHandle {
    pub addr: SocketAddr,
    pub spki_sha256: String,
    handle: axum_server::Handle<SocketAddr>,
    _task: tokio::task::JoinHandle<()>,
}

impl LanSyncServerHandle {
    pub fn shutdown(self) {
        self.handle.graceful_shutdown(Some(Duration::from_secs(5)));
    }

    pub fn info(&self) -> LanServerInfo {
        LanServerInfo {
            port: self.addr.port(),
            spki_sha256: self.spki_sha256.clone(),
        }
    }
}

type SharedLanServerState = ServerState<LanManifestStore, LanServerPeerStore>;

pub async fn spawn_lan_sync_server(
    addr: SocketAddr,
    sync_root: PathBuf,
    store: LanPeerStore,
    inbound: Arc<dyn LanInboundRequestHandler>,
) -> Result<LanSyncServerHandle, DomainError> {
    let identity = store.load_or_create_identity().await?;
    let tls = SelfManagedTls::load_or_create(&store.state_dir()).map_err(sync_error_to_domain)?;
    let spki_sha256 = tls.spki_sha256().to_string();

    let manifest_store = Arc::new(LanManifestStore::new(sync_root));
    let peer_store = Arc::new(LanServerPeerStore::new(store.clone()));
    let session_manager = Arc::new(SessionManager::new(SessionManagerConfig::default()));

    let mut status = default_status_response();
    status.protocol = "lan-v2".to_string();
    status.server = "tauritavern-lan".to_string();
    status.device_id = Some(identity.device_id.clone());
    status.device_name = Some(identity.device_name.clone());
    status.spki_sha256 = Some(spki_sha256.clone());
    append_feature(&mut status.features, LAN_HTTPS_FEATURE_V1);
    append_feature(&mut status.features, LAN_SESSION_FEATURE_V1);
    append_feature(&mut status.features, LAN_PULL_REQUEST_SELECTION_FEATURE_V1);

    let shared_state = Arc::new(
        ServerState::new(
            identity.device_id.clone(),
            identity.device_name.clone(),
            manifest_store,
            peer_store,
            session_manager,
        )
        .with_status(status),
    );
    let lan_state = Arc::new(LanServerState {
        inbound,
        shared: shared_state.clone(),
    });

    let app = build_transfer_router(shared_state).merge(
        Router::new()
            .route("/v2/lan/pair/complete", post(handle_lan_pair_complete))
            .route("/v2/lan/pull-request", post(handle_pull_request))
            .with_state(lan_state),
    );

    spawn_router(addr, Arc::new(tls), spki_sha256, app).await
}

async fn spawn_router(
    addr: SocketAddr,
    tls: Arc<dyn TlsProvider>,
    spki_sha256: String,
    app: Router,
) -> Result<LanSyncServerHandle, DomainError> {
    let server_config = tls.server_config().map_err(sync_error_to_domain)?;
    let tls_config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config));

    let listener = std::net::TcpListener::bind(addr)
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let handle = axum_server::Handle::<SocketAddr>::new();
    let mut server = axum_server::from_tcp_rustls(listener, tls_config)
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .handle(handle.clone());
    server
        .http_builder()
        .http2()
        .max_concurrent_streams(Some(256))
        .initial_connection_window_size(Some(4 * 1024 * 1024))
        .initial_stream_window_size(Some(1024 * 1024));

    let task = tokio::spawn(async move {
        if let Err(error) = server.serve(app.into_make_service()).await {
            tracing::error!(
                target: crate::observability_targets::USER_VISIBLE_ERROR,
                "LAN Sync server failed: {}",
                error
            );
        }
    });

    Ok(LanSyncServerHandle {
        addr,
        spki_sha256,
        handle,
        _task: task,
    })
}

#[derive(Clone)]
struct LanManifestStore {
    sync_root: PathBuf,
}

impl LanManifestStore {
    fn new(sync_root: PathBuf) -> Self {
        Self { sync_root }
    }
}

impl ManifestStore for LanManifestStore {
    fn scan(
        &self,
        policy: ResolvedDatasetPolicy,
    ) -> impl std::future::Future<Output = Result<ttsync_contract::manifest::ManifestV2, SyncError>> + Send
    {
        let sync_root = self.sync_root.clone();
        async move {
            scan_manifest_with_policy(sync_root, policy)
                .await
                .map_err(domain_error_to_sync)
        }
    }

    fn read_file(
        &self,
        path: &ttsync_contract::path::SyncPath,
    ) -> impl std::future::Future<Output = Result<Box<dyn AsyncRead + Send + Unpin>, SyncError>> + Send
    {
        let sync_root = self.sync_root.clone();
        let path = path.clone();
        async move {
            let full_path = sync_transfer::resolve_to_local(&sync_root, &path);
            let file = tokio::fs::File::open(&full_path)
                .await
                .map_err(|error| SyncError::Io(error.to_string()))?;
            Ok(Box::new(file) as Box<dyn AsyncRead + Send + Unpin>)
        }
    }

    fn write_file(
        &self,
        path: &ttsync_contract::path::SyncPath,
        data: &mut (dyn AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let sync_root = self.sync_root.clone();
        let path = path.clone();
        async move {
            let full_path = sync_transfer::resolve_to_local(&sync_root, &path);
            sync_fs::write_file_atomic(&full_path, data, modified_ms)
                .await
                .map_err(|error| domain_error_to_sync(error.into_error()))
        }
    }

    fn delete_file(
        &self,
        path: &ttsync_contract::path::SyncPath,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let sync_root = self.sync_root.clone();
        let path = path.clone();
        async move {
            let full_path = sync_transfer::resolve_to_local(&sync_root, &path);
            tokio::fs::remove_file(&full_path)
                .await
                .map_err(|error| SyncError::Io(error.to_string()))
        }
    }
}

#[derive(Clone)]
struct LanServerPeerStore {
    store: LanPeerStore,
}

impl LanServerPeerStore {
    fn new(store: LanPeerStore) -> Self {
        Self { store }
    }
}

impl PeerStore for LanServerPeerStore {
    fn get_peer(
        &self,
        device_id: &DeviceId,
    ) -> impl std::future::Future<Output = Result<PeerGrant, SyncError>> + Send {
        let store = self.store.clone();
        let device_id = device_id.clone();
        async move {
            store
                .get_peer_grant(&device_id)
                .await
                .map_err(domain_error_to_sync)
        }
    }

    fn save_peer(
        &self,
        grant: PeerGrant,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let store = self.store.clone();
        async move {
            store
                .save_peer_grant(grant)
                .await
                .map_err(domain_error_to_sync)
        }
    }

    fn remove_peer(
        &self,
        device_id: &DeviceId,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let store = self.store.clone();
        let device_id = device_id.clone();
        async move {
            store
                .remove_paired_device(&device_id)
                .await
                .map_err(domain_error_to_sync)
        }
    }

    fn list_peers(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<PeerGrant>, SyncError>> + Send {
        let store = self.store.clone();
        async move {
            store
                .load_paired_devices()
                .await
                .map(|devices| {
                    devices
                        .into_iter()
                        .map(|device| device.grant)
                        .collect::<Vec<_>>()
                })
                .map_err(domain_error_to_sync)
        }
    }
}

struct LanServerState {
    inbound: Arc<dyn LanInboundRequestHandler>,
    shared: Arc<SharedLanServerState>,
}

#[derive(Debug, serde::Deserialize)]
struct PairQuery {
    token: String,
}

async fn handle_lan_pair_complete(
    State(state): State<Arc<LanServerState>>,
    Query(query): Query<PairQuery>,
    Json(request): Json<LanPairCompleteRequest>,
) -> Result<Json<LanPairCompleteResponse>, ApiError> {
    let response = state
        .inbound
        .complete_pairing(query.token, request)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(response))
}

async fn handle_pull_request(
    State(state): State<Arc<LanServerState>>,
    headers: HeaderMap,
    request: Result<Json<SyncOperationOptions>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let peer = state
        .shared
        .authenticate_headers(&headers)
        .await
        .map_err(ApiError::from)?;
    let Json(options) = request.map_err(|error| {
        ApiError::from(DomainError::InvalidData(format!(
            "Invalid LAN Sync pull request: {error}"
        )))
    })?;
    let options = options.validate()?;
    state
        .inbound
        .accept_pull_request(peer.device_id, options)
        .await
        .map_err(ApiError::from)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "ok": true,
        })),
    ))
}

#[derive(Debug)]
struct ApiError {
    error: DomainError,
}

impl From<DomainError> for ApiError {
    fn from(error: DomainError) -> Self {
        Self { error }
    }
}

impl From<SyncError> for ApiError {
    fn from(error: SyncError) -> Self {
        Self::from(sync_error_to_domain(error))
    }
}

#[cfg(test)]
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self.error {
            DomainError::NotFound(message) => (StatusCode::NOT_FOUND, message),
            DomainError::InvalidData(message) => (StatusCode::BAD_REQUEST, message),
            DomainError::AuthenticationError(message) if message == PAIRING_REJECTED_MESSAGE => {
                (StatusCode::FORBIDDEN, message)
            }
            DomainError::AuthenticationError(message) => (StatusCode::UNAUTHORIZED, message),
            DomainError::Cancelled(message) => (StatusCode::SERVICE_UNAVAILABLE, message),
            DomainError::InternalError(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
            DomainError::RateLimited { message } => (StatusCode::TOO_MANY_REQUESTS, message),
            DomainError::Transient(message) => (StatusCode::SERVICE_UNAVAILABLE, message),
            DomainError::UpstreamFailure(failure) => {
                (StatusCode::SERVICE_UNAVAILABLE, failure.to_string())
            }
            DomainError::WorkspacePathIsDirectory { path } => (
                StatusCode::CONFLICT,
                format!("Workspace path is a directory: {path}"),
            ),
            DomainError::WorkspaceWriteConflict { kind, .. } => (
                StatusCode::CONFLICT,
                format!("Workspace write conflict: {kind}"),
            ),
        };
        (
            status,
            Json(json!({
                "ok": false,
                "error": message,
            })),
        )
            .into_response()
    }
}

fn append_feature(features: &mut Vec<String>, feature: &str) {
    if !features.iter().any(|item| item == feature) {
        features.push(feature.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use async_trait::async_trait;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ttsync_client::{ClientSyncEngine, ClientSyncOptions, ClientSyncTarget, NoopSyncObserver};
    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DATASET_SCOPE_FEATURE_V1};
    use ttsync_contract::manifest::ManifestV2;
    use ttsync_contract::peer::Permissions;
    use ttsync_contract::sync::SyncMode;
    use ttsync_core::bundle::{FEATURE_BUNDLE_V1, FEATURE_ZSTD_V1};
    use ttsync_core::dataset::tauri_tavern_default_selection;
    use uuid::Uuid;

    use crate::domain::models::lan_sync::LanSyncPairedDevice;
    use crate::infrastructure::sync::http_client::{bearer_auth_value, new_sync_client};
    use crate::infrastructure::sync::lan::client::{LanSyncClient, complete_pairing};
    use crate::infrastructure::sync::workspace::TauriTavernSyncWorkspace;

    fn temp_default_user_dir() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-lan-server-{}", Uuid::new_v4()))
    }

    struct NoopInboundHandler;

    #[async_trait]
    impl LanInboundRequestHandler for NoopInboundHandler {
        async fn complete_pairing(
            &self,
            _token: String,
            _request: LanPairCompleteRequest,
        ) -> Result<LanPairCompleteResponse, DomainError> {
            Err(DomainError::AuthenticationError(
                "Pairing not enabled".to_string(),
            ))
        }

        async fn accept_pull_request(
            &self,
            _peer_device_id: DeviceId,
            _options: SyncOperationOptions,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct RecordingPairingInboundHandler {
        token: String,
        requests: std::sync::Mutex<Vec<(String, LanPairCompleteRequest)>>,
    }

    #[async_trait]
    impl LanInboundRequestHandler for RecordingPairingInboundHandler {
        async fn complete_pairing(
            &self,
            token: String,
            request: LanPairCompleteRequest,
        ) -> Result<LanPairCompleteResponse, DomainError> {
            if token != self.token {
                return Err(DomainError::AuthenticationError(
                    "Invalid pairing token".to_string(),
                ));
            }
            self.requests
                .lock()
                .expect("pairing request lock")
                .push((token, request));

            Ok(LanPairCompleteResponse {
                server_device_id: DeviceId::new("11111111-1111-4111-8111-111111111111".to_string())
                    .unwrap(),
                server_device_name: "Server".to_string(),
                server_device_pubkey: URL_SAFE_NO_PAD.encode([8u8; 32]),
                granted_permissions: Permissions {
                    read: true,
                    write: false,
                    mirror_delete: true,
                },
            })
        }

        async fn accept_pull_request(
            &self,
            _peer_device_id: DeviceId,
            _options: SyncOperationOptions,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn noop_inbound() -> Arc<dyn LanInboundRequestHandler> {
        Arc::new(NoopInboundHandler)
    }

    #[test]
    fn pairing_rejection_maps_to_forbidden() {
        let response = ApiError::from(DomainError::AuthenticationError(
            PAIRING_REJECTED_MESSAGE.to_string(),
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn cancelled_pairing_maps_to_service_unavailable() {
        let response =
            ApiError::from(DomainError::cancelled("Pairing request cancelled")).into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn status_is_served_over_spki_pinned_https() {
        let default_user_dir = temp_default_user_dir();
        let store = LanPeerStore::new(default_user_dir.clone());
        let handle = spawn_lan_sync_server(
            "127.0.0.1:0".parse().unwrap(),
            default_user_dir.clone(),
            store,
            noop_inbound(),
        )
        .await
        .expect("spawn LAN Sync server");

        let api = LanSyncClient::new(
            format!("https://127.0.0.1:{}", handle.addr.port()),
            handle.spki_sha256.clone(),
        )
        .expect("pinned api");

        let status = api.status().await.expect("status");
        assert!(status.ok);
        assert_eq!(status.protocol, "lan-v2");
        assert_eq!(status.dataset_policy_version, Some(DATASET_POLICY_VERSION));
        assert!(
            status
                .features
                .iter()
                .any(|item| item == LAN_HTTPS_FEATURE_V1)
        );
        assert!(
            status
                .features
                .iter()
                .any(|item| item == LAN_SESSION_FEATURE_V1)
        );
        assert!(
            status
                .features
                .iter()
                .any(|item| item == LAN_PULL_REQUEST_SELECTION_FEATURE_V1)
        );
        assert!(
            status
                .features
                .iter()
                .any(|item| item == DATASET_SCOPE_FEATURE_V1)
        );
        assert!(status.features.iter().any(|item| item == FEATURE_BUNDLE_V1));
        assert!(status.features.iter().any(|item| item == FEATURE_ZSTD_V1));

        handle.shutdown();
        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pair_complete_delegates_to_inbound_handler() {
        let default_user_dir = temp_default_user_dir();
        let store = LanPeerStore::new(default_user_dir.clone());
        let token = "pair-token";
        let inbound = Arc::new(RecordingPairingInboundHandler {
            token: token.to_string(),
            requests: std::sync::Mutex::new(Vec::new()),
        });
        let handle = spawn_lan_sync_server(
            "127.0.0.1:0".parse().unwrap(),
            default_user_dir.clone(),
            store.clone(),
            inbound.clone(),
        )
        .await
        .expect("spawn LAN Sync server");

        let peer_device_id =
            DeviceId::new("550e8400-e29b-41d4-a716-446655440000".to_string()).unwrap();
        let response = complete_pairing(
            &format!("https://127.0.0.1:{}", handle.addr.port()),
            &handle.spki_sha256,
            token,
            &LanPairCompleteRequest {
                device_id: peer_device_id.clone(),
                device_name: "Peer".to_string(),
                device_pubkey: URL_SAFE_NO_PAD.encode([9u8; 32]),
                client_base_url: "https://127.0.0.1:60000".to_string(),
                client_spki_sha256: "client-spki".to_string(),
            },
        )
        .await
        .expect("complete pair");

        assert!(response.granted_permissions.read);
        assert!(response.granted_permissions.mirror_delete);
        assert!(!response.granted_permissions.write);
        assert_eq!(response.server_device_name, "Server");

        let requests = inbound.requests.lock().expect("pairing request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, token);
        assert_eq!(requests[0].1.device_id, peer_device_id);
        assert_eq!(requests[0].1.client_base_url, "https://127.0.0.1:60000");
        assert_eq!(requests[0].1.client_spki_sha256, "client-spki");

        handle.shutdown();
        let _ = tokio::fs::remove_dir_all(default_user_dir).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pull_plan_and_file_download_use_session_and_dataset_scope() {
        let sync_root = temp_default_user_dir();
        tokio::fs::create_dir_all(sync_root.join("default-user/chats"))
            .await
            .expect("create sync scope");
        tokio::fs::write(
            sync_root.join("default-user/chats/hello.json"),
            br#"{"hello":true}"#,
        )
        .await
        .expect("write source file");

        let store = LanPeerStore::new(sync_root.clone());
        let peer_device_id =
            DeviceId::new("550e8400-e29b-41d4-a716-446655440001".to_string()).unwrap();
        let peer_seed = URL_SAFE_NO_PAD.encode([3u8; 32]);
        let peer_pubkey = URL_SAFE_NO_PAD
            .decode(ttsync_core::crypto::device_pubkey_b64url(&peer_seed).unwrap())
            .unwrap();
        store
            .upsert_paired_device(LanSyncPairedDevice {
                grant: PeerGrant {
                    device_id: peer_device_id.clone(),
                    device_name: "Peer".to_string(),
                    public_key: peer_pubkey,
                    permissions: Permissions {
                        read: true,
                        write: false,
                        mirror_delete: true,
                    },
                    paired_at_ms: now_ms(),
                    last_sync_ms: None,
                },
                base_url: "https://127.0.0.1:60000".to_string(),
                spki_sha256: "peer-spki".to_string(),
            })
            .await
            .expect("store peer");

        let handle = spawn_lan_sync_server(
            "127.0.0.1:0".parse().unwrap(),
            sync_root.clone(),
            store,
            noop_inbound(),
        )
        .await
        .expect("spawn LAN Sync server");

        let api = LanSyncClient::new(
            format!("https://127.0.0.1:{}", handle.addr.port()),
            handle.spki_sha256.clone(),
        )
        .expect("pinned api");
        let status = api.status().await.expect("status");
        crate::infrastructure::sync::http_client::ensure_dataset_scope_v1(&status, "LAN Sync peer")
            .expect("dataset scope feature");
        let session = api
            .open_session(&peer_device_id, &peer_seed)
            .await
            .expect("open session");
        let client = new_sync_client(
            format!("https://127.0.0.1:{}", handle.addr.port()),
            handle.spki_sha256.clone(),
        )
        .expect("shared client");
        let pull_request_url = client
            .endpoint_url("/v2/lan/pull-request")
            .expect("pull request url");
        let auth = bearer_auth_value(&session.session_token);
        let missing_body_status = client
            .http()
            .post(pull_request_url.clone())
            .header(reqwest::header::AUTHORIZATION, auth.clone())
            .send()
            .await
            .expect("send missing body")
            .status();
        assert_eq!(missing_body_status, StatusCode::BAD_REQUEST);
        let missing_selection_status = client
            .http()
            .post(pull_request_url)
            .header(reqwest::header::AUTHORIZATION, auth)
            .json(&json!({"require_bundle_zstd": true}))
            .send()
            .await
            .expect("send missing selection")
            .status();
        assert_eq!(missing_selection_status, StatusCode::BAD_REQUEST);

        let plan = api
            .pull_plan(
                &session.session_token,
                SyncMode::Incremental,
                tauri_tavern_default_selection(),
                ManifestV2 { entries: vec![] },
            )
            .await
            .expect("pull plan");

        assert_eq!(plan.files_total, 1);
        assert_eq!(plan.transfer.len(), 1);
        assert_eq!(
            plan.transfer[0].path.as_str(),
            "default-user/chats/hello.json"
        );

        let response = api
            .download_file(
                &session.session_token,
                &plan.plan_id,
                &plan.transfer[0].path,
            )
            .await
            .expect("download file");
        let bytes = response.bytes().await.expect("download bytes");
        assert_eq!(&bytes[..], br#"{"hello":true}"#);

        let bundle_response = api
            .download_bundle(&session.session_token, &plan.plan_id, true)
            .await
            .expect("download bundle");
        let content_encoding = bundle_response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert_eq!(content_encoding, "zstd");

        let target_root = temp_default_user_dir();
        tokio::fs::create_dir_all(&target_root)
            .await
            .expect("create target root");
        let workspace = Arc::new(TauriTavernSyncWorkspace::new(target_root.clone()));
        let mut options =
            ClientSyncOptions::new(SyncMode::Incremental, tauri_tavern_default_selection());
        options.require_bundle_zstd = true;
        let report = ClientSyncEngine::new(
            client,
            workspace,
            ClientSyncTarget {
                device_id: peer_device_id,
                ed25519_seed_b64url: peer_seed,
            },
            "LAN Sync peer",
        )
        .pull(options, &NoopSyncObserver)
        .await
        .expect("shared client pull");
        assert_eq!(report.summary.files_total, 1);
        assert_eq!(report.local_applied.files_written, 1);
        let bundle_bytes = tokio::fs::read(target_root.join("default-user/chats/hello.json"))
            .await
            .expect("read bundle file");
        assert_eq!(&bundle_bytes, br#"{"hello":true}"#);

        handle.shutdown();
        let _ = tokio::fs::remove_dir_all(target_root).await;
        let _ = tokio::fs::remove_dir_all(sync_root).await;
    }
}
