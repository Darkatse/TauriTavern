use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Query, State},
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

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{LanSyncIdentity, LanSyncPairedDevice};
use crate::domain::models::sync::SyncOperationOptions;
use crate::infrastructure::sync::http_client::{domain_error_to_sync, sync_error_to_domain};
use crate::infrastructure::sync::lan::pairing::{
    LanPairCompleteRequest, LanPairCompleteResponse, LanPairingCoordinator,
    decode_device_pubkey_b64url, default_lan_permissions, host_for_pairing_prompt,
    validate_https_base_url,
};
use crate::infrastructure::sync::lan::store::LanPeerStore;
use crate::infrastructure::sync_fs;
use crate::infrastructure::sync_transfer;
use crate::infrastructure::tt_sync::fs::scan_manifest_with_policy;

const LAN_HTTPS_FEATURE_V1: &str = "lan_https_v1";
const LAN_SESSION_FEATURE_V1: &str = "lan_session_v1";
pub(crate) const LAN_PULL_REQUEST_SELECTION_FEATURE_V1: &str = "lan_pull_request_selection_v1";

#[async_trait]
pub trait LanPullRequestHandler: Send + Sync {
    async fn accept_pull_request(
        &self,
        peer_device_id: DeviceId,
        options: SyncOperationOptions,
    ) -> Result<(), DomainError>;
}

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
}

type SharedLanServerState = ServerState<LanManifestStore, LanServerPeerStore>;

pub async fn spawn_lan_sync_server(
    addr: SocketAddr,
    sync_root: PathBuf,
    store: LanPeerStore,
    pairing: Arc<dyn LanPairingCoordinator>,
    pull_requests: Arc<dyn LanPullRequestHandler>,
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
        identity,
        store,
        pairing,
        pull_requests,
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
            tracing::error!("LAN Sync server failed: {}", error);
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
    identity: LanSyncIdentity,
    store: LanPeerStore,
    pairing: Arc<dyn LanPairingCoordinator>,
    pull_requests: Arc<dyn LanPullRequestHandler>,
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
    let session = state
        .pairing
        .active_pairing_session()
        .await
        .ok_or_else(|| ApiError::unauthorized("Pairing not enabled"))?;

    if query.token != session.token {
        return Err(ApiError::unauthorized("Invalid pairing token"));
    }
    if now_ms() > session.expires_at_ms {
        return Err(ApiError::unauthorized("Pairing expired"));
    }
    if request.device_id == state.identity.device_id {
        return Err(ApiError::invalid_data(
            "Cannot pair LAN Sync device with itself",
        ));
    }

    validate_https_base_url(&request.client_base_url).map_err(ApiError::from)?;
    if request.client_spki_sha256.trim().is_empty() {
        return Err(ApiError::invalid_data("Missing LAN Sync client SPKI"));
    }
    let public_key = decode_device_pubkey_b64url(&request.device_pubkey).map_err(ApiError::from)?;

    let peer_ip = host_for_pairing_prompt(&request.client_base_url).map_err(ApiError::from)?;
    let accepted = state
        .pairing
        .request_pairing_decision(
            request.device_id.to_string(),
            request.device_name.clone(),
            peer_ip,
        )
        .await
        .map_err(ApiError::from)?;

    if !accepted {
        return Err(ApiError::forbidden("Pairing rejected"));
    }

    let permissions = default_lan_permissions();
    let paired_at_ms = now_ms();
    state
        .store
        .upsert_paired_device(LanSyncPairedDevice {
            grant: PeerGrant {
                device_id: request.device_id,
                device_name: request.device_name,
                public_key,
                permissions,
                paired_at_ms,
                last_sync_ms: None,
            },
            base_url: request.client_base_url,
            spki_sha256: request.client_spki_sha256,
        })
        .await
        .map_err(ApiError::from)?;

    state.pairing.clear_pairing_session().await;

    let server_device_pubkey =
        ttsync_core::crypto::device_pubkey_b64url(&state.identity.ed25519_seed)
            .map_err(ApiError::from)?;
    Ok(Json(LanPairCompleteResponse {
        server_device_id: state.identity.device_id.clone(),
        server_device_name: state.identity.device_name.clone(),
        server_device_pubkey,
        granted_permissions: permissions,
    }))
}

async fn handle_pull_request(
    State(state): State<Arc<LanServerState>>,
    headers: HeaderMap,
    request: Option<Json<SyncOperationOptions>>,
) -> Result<impl IntoResponse, ApiError> {
    let peer = state
        .shared
        .authenticate_headers(&headers)
        .await
        .map_err(ApiError::from)?;
    let options = request
        .map(|Json(value)| value)
        .unwrap_or_default()
        .validate()?;
    state
        .pull_requests
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
    status_override: Option<StatusCode>,
    error: DomainError,
}

impl ApiError {
    fn invalid_data(message: impl Into<String>) -> Self {
        Self::from(DomainError::InvalidData(message.into()))
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self::from(DomainError::AuthenticationError(message.into()))
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status_override: Some(StatusCode::FORBIDDEN),
            error: DomainError::AuthenticationError(message.into()),
        }
    }
}

impl From<DomainError> for ApiError {
    fn from(error: DomainError) -> Self {
        Self {
            status_override: None,
            error,
        }
    }
}

impl From<SyncError> for ApiError {
    fn from(error: SyncError) -> Self {
        Self::from(sync_error_to_domain(error))
    }
}

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
            DomainError::AuthenticationError(message) => (StatusCode::UNAUTHORIZED, message),
            DomainError::Cancelled(message) => (StatusCode::from_u16(499).unwrap(), message),
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
        let status = self.status_override.unwrap_or(status);

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

    use async_compression::tokio::bufread::ZstdDecoder;
    use async_trait::async_trait;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use futures_util::TryStreamExt;
    use tokio::io::BufReader;
    use tokio_util::io::StreamReader;
    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DATASET_SCOPE_FEATURE_V1};
    use ttsync_contract::manifest::ManifestV2;
    use ttsync_contract::peer::Permissions;
    use ttsync_contract::sync::SyncMode;
    use ttsync_core::dataset::tauri_tavern_default_selection;
    use uuid::Uuid;

    use crate::infrastructure::sync::lan::client::{LanSyncClient, complete_pairing};
    use crate::infrastructure::sync::lan::pairing::{LanPairingCoordinator, LanPairingSession};
    use crate::infrastructure::sync_bundle::{
        BUNDLE_ZSTD_DECODE_BUFFER_SIZE, FEATURE_BUNDLE_V1, FEATURE_ZSTD_V1,
        write_bundle_to_local_files,
    };

    fn temp_default_user_dir() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-lan-server-{}", Uuid::new_v4()))
    }

    struct TestPairingCoordinator {
        session: Option<LanPairingSession>,
        accept: bool,
    }

    struct NoopPullRequestHandler;

    #[async_trait]
    impl LanPullRequestHandler for NoopPullRequestHandler {
        async fn accept_pull_request(
            &self,
            _peer_device_id: DeviceId,
            _options: SyncOperationOptions,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[async_trait]
    impl LanPairingCoordinator for TestPairingCoordinator {
        async fn active_pairing_session(&self) -> Option<LanPairingSession> {
            self.session.clone()
        }

        async fn request_pairing_decision(
            &self,
            _peer_device_id: String,
            _peer_device_name: String,
            _peer_ip: String,
        ) -> Result<bool, DomainError> {
            Ok(self.accept)
        }

        async fn clear_pairing_session(&self) {}
    }

    fn inactive_pairing() -> Arc<dyn LanPairingCoordinator> {
        Arc::new(TestPairingCoordinator {
            session: None,
            accept: false,
        })
    }

    fn accepting_pairing(token: &str) -> Arc<dyn LanPairingCoordinator> {
        Arc::new(TestPairingCoordinator {
            session: Some(LanPairingSession {
                token: token.to_string(),
                expires_at_ms: now_ms() + 60_000,
            }),
            accept: true,
        })
    }

    fn noop_pull_requests() -> Arc<dyn LanPullRequestHandler> {
        Arc::new(NoopPullRequestHandler)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn status_is_served_over_spki_pinned_https() {
        let default_user_dir = temp_default_user_dir();
        let store = LanPeerStore::new(default_user_dir.clone());
        let handle = spawn_lan_sync_server(
            "127.0.0.1:0".parse().unwrap(),
            default_user_dir.clone(),
            store,
            inactive_pairing(),
            noop_pull_requests(),
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
    async fn pair_complete_stores_peer_grant() {
        let default_user_dir = temp_default_user_dir();
        let store = LanPeerStore::new(default_user_dir.clone());
        let token = "pair-token";
        let handle = spawn_lan_sync_server(
            "127.0.0.1:0".parse().unwrap(),
            default_user_dir.clone(),
            store.clone(),
            accepting_pairing(token),
            noop_pull_requests(),
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
        assert!(!response.server_device_pubkey.is_empty());

        let peer = store
            .get_paired_device(&peer_device_id)
            .await
            .expect("stored peer");
        assert_eq!(peer.base_url, "https://127.0.0.1:60000");
        assert_eq!(peer.spki_sha256, "client-spki");
        assert_eq!(peer.grant.public_key, vec![9u8; 32]);

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
            inactive_pairing(),
            noop_pull_requests(),
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
        let stream = bundle_response
            .bytes_stream()
            .map_err(std::io::Error::other);
        let reader = StreamReader::new(stream);
        let mut reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(ZstdDecoder::new(
            BufReader::with_capacity(BUNDLE_ZSTD_DECODE_BUFFER_SIZE, reader),
        ));
        let mut progress_paths = Vec::new();
        write_bundle_to_local_files(
            &target_root,
            plan.transfer.clone(),
            &mut reader,
            |progress| {
                progress_paths.push(progress.path);
                Ok(())
            },
        )
        .await
        .expect("write bundle files");
        assert_eq!(
            progress_paths,
            vec!["default-user/chats/hello.json".to_string()]
        );
        let bundle_bytes = tokio::fs::read(target_root.join("default-user/chats/hello.json"))
            .await
            .expect("read bundle file");
        assert_eq!(&bundle_bytes, br#"{"hello":true}"#);

        handle.shutdown();
        let _ = tokio::fs::remove_dir_all(target_root).await;
        let _ = tokio::fs::remove_dir_all(sync_root).await;
    }
}
