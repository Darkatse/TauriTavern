use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, mpsc, oneshot};
use ttsync_contract::peer::DeviceId;
use ttsync_contract::sync::SyncMode;

use super::pairing_link::{default_lan_permissions, device_pubkey_b64url};
use super::*;
use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::application::services::sync_job_coordinator::SyncJobExecutor;
use crate::domain::models::lan_sync::{
    LanPairCompleteRequest, LanPairCompleteResponse, LanServerSettings, LanSyncIdentity,
    LanSyncPairedDevice, LanSyncSyncCompletedEvent, LanSyncSyncErrorEvent,
    LanSyncSyncProgressEvent, SyncPreferences,
};
use crate::domain::models::sync::{
    LocalAppliedChangeSummary, ResolvedSyncPolicy, SyncEndpointRef, SyncExecutionFailure,
    SyncExecutionKind, SyncExecutionReport, SyncIntent, SyncJob, SyncJobSummary,
};

struct MemorySettingsRepository {
    manual_default_mode: SyncMode,
}

#[async_trait]
impl LanSyncSettingsRepository for MemorySettingsRepository {
    async fn load_or_create_server_settings(&self) -> Result<LanServerSettings, DomainError> {
        Ok(LanServerSettings {
            port: 51_234,
            auto_start: false,
        })
    }

    async fn load_or_create_sync_preferences(&self) -> Result<SyncPreferences, DomainError> {
        Ok(SyncPreferences {
            manual_default_mode: self.manual_default_mode,
        })
    }

    async fn save_sync_preferences(
        &self,
        _preferences: &SyncPreferences,
    ) -> Result<(), DomainError> {
        Ok(())
    }
}

struct MemoryPeerRepository {
    identity: LanSyncIdentity,
    paired_devices: Mutex<Vec<LanSyncPairedDevice>>,
}

#[async_trait]
impl LanPeerRepository for MemoryPeerRepository {
    async fn load_or_create_identity(&self) -> Result<LanSyncIdentity, DomainError> {
        Ok(self.identity.clone())
    }

    async fn load_paired_devices(&self) -> Result<Vec<LanSyncPairedDevice>, DomainError> {
        Ok(self.paired_devices.lock().await.clone())
    }

    async fn upsert_paired_device(&self, device: LanSyncPairedDevice) -> Result<(), DomainError> {
        let mut devices = self.paired_devices.lock().await;
        devices.retain(|existing| existing.grant.device_id != device.grant.device_id);
        devices.push(device);
        Ok(())
    }

    async fn remove_paired_device(&self, device_id: &DeviceId) -> Result<(), DomainError> {
        self.paired_devices
            .lock()
            .await
            .retain(|device| &device.grant.device_id != device_id);
        Ok(())
    }
}

struct StaticApproval {
    accept: bool,
    requests: Mutex<Vec<LanPairingApprovalRequest>>,
}

#[async_trait]
impl PairingApproval for StaticApproval {
    async fn request(&self, request: LanPairingApprovalRequest) -> Result<bool, DomainError> {
        self.requests.lock().await.push(request);
        Ok(self.accept)
    }

    async fn confirm(&self, _request_id: &str, _accept: bool) -> Result<(), DomainError> {
        Ok(())
    }

    async fn cancel_all(&self) {}
}

struct NoopEvents;

impl LanSyncEventPublisher for NoopEvents {
    fn publish_progress(&self, _payload: LanSyncSyncProgressEvent) {}
    fn publish_completed(&self, _payload: LanSyncSyncCompletedEvent) {}
    fn publish_error(&self, _payload: LanSyncSyncErrorEvent) {}
}

struct RecordingEvents {
    completed: mpsc::UnboundedSender<LanSyncSyncCompletedEvent>,
}

impl LanSyncEventPublisher for RecordingEvents {
    fn publish_progress(&self, _payload: LanSyncSyncProgressEvent) {}

    fn publish_completed(&self, payload: LanSyncSyncCompletedEvent) {
        self.completed.send(payload).expect("record completion");
    }

    fn publish_error(&self, _payload: LanSyncSyncErrorEvent) {}
}

struct RecordingExecutor {
    jobs: mpsc::UnboundedSender<SyncJob>,
}

#[async_trait]
impl SyncJobExecutor for RecordingExecutor {
    async fn execute(&self, job: SyncJob) -> Result<SyncExecutionReport, SyncExecutionFailure> {
        self.jobs.send(job).expect("record sync job");
        Ok(SyncExecutionReport::completed(
            SyncJobSummary::new(0, 0, 0),
            LocalAppliedChangeSummary::default(),
        ))
    }
}

struct BlockingExecutor {
    started: mpsc::UnboundedSender<()>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}

#[async_trait]
impl SyncJobExecutor for BlockingExecutor {
    async fn execute(&self, _job: SyncJob) -> Result<SyncExecutionReport, SyncExecutionFailure> {
        self.started.send(()).expect("record started job");
        let release = self.release.lock().await.take().expect("release receiver");
        let _ = release.await;
        Ok(SyncExecutionReport::completed(
            SyncJobSummary::new(1, 2, 3),
            LocalAppliedChangeSummary::default(),
        ))
    }
}

struct NoopReconciler;

#[async_trait]
impl DataChangeReconciler for NoopReconciler {
    async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

struct MemoryServerControl;

#[async_trait]
impl LanServerControl for MemoryServerControl {
    async fn start(&self, _port: u16) -> Result<LanServerInfo, DomainError> {
        Ok(LanServerInfo {
            port: 51_234,
            spki_sha256: "server-spki".to_string(),
        })
    }

    async fn stop(&self) -> Result<(), DomainError> {
        Ok(())
    }

    async fn running_info(&self) -> Option<LanServerInfo> {
        Some(LanServerInfo {
            port: 51_234,
            spki_sha256: "server-spki".to_string(),
        })
    }
}

struct NoopAddressDiscovery;

#[async_trait]
impl LanAddressDiscovery for NoopAddressDiscovery {
    fn list_available_addresses(&self, _port: u16) -> Result<Vec<String>, DomainError> {
        Ok(Vec::new())
    }

    fn default_advertise_address(
        &self,
        _port: u16,
        _available_addresses: &[String],
    ) -> Option<String> {
        None
    }

    async fn routed_advertise_address(
        &self,
        _peer_base_url: &str,
        _local_port: u16,
    ) -> Result<String, DomainError> {
        Err(DomainError::InternalError("not used".to_string()))
    }
}

struct NoopPairingClient;

#[async_trait]
impl LanPairingClient for NoopPairingClient {
    async fn complete_pairing(
        &self,
        _base_url: &str,
        _spki_sha256: &str,
        _token: &str,
        _request: &LanPairCompleteRequest,
    ) -> Result<LanPairCompleteResponse, DomainError> {
        Err(DomainError::InternalError("not used".to_string()))
    }
}

struct ReplacingApproval {
    state: Arc<LanSyncRuntimeState>,
}

#[async_trait]
impl PairingApproval for ReplacingApproval {
    async fn request(&self, _request: LanPairingApprovalRequest) -> Result<bool, DomainError> {
        self.state
            .set_pairing_session(LanPairingSession {
                token: "new-token".to_string(),
                expires_at_ms: now_ms() + 60_000,
            })
            .await;
        Ok(true)
    }

    async fn confirm(&self, _request_id: &str, _accept: bool) -> Result<(), DomainError> {
        Ok(())
    }

    async fn cancel_all(&self) {}
}

fn test_device_id(value: &str) -> DeviceId {
    DeviceId::new(value.to_string()).expect("valid device id")
}

fn test_identity(device_id: DeviceId, device_name: &str) -> LanSyncIdentity {
    LanSyncIdentity {
        device_id,
        device_name: device_name.to_string(),
        ed25519_seed: ttsync_core::crypto::random_base64url(32),
    }
}

fn peer_request(device_id: DeviceId, device_name: &str) -> LanPairCompleteRequest {
    let seed = ttsync_core::crypto::random_base64url(32);
    LanPairCompleteRequest {
        device_id,
        device_name: device_name.to_string(),
        device_pubkey: device_pubkey_b64url(&seed).expect("peer public key"),
        client_base_url: "https://192.168.1.23:51000".to_string(),
        client_spki_sha256: "peer-spki".to_string(),
    }
}

fn inbound_service(
    state: Arc<LanSyncRuntimeState>,
    peer_repository: Arc<MemoryPeerRepository>,
    approval: Arc<StaticApproval>,
    jobs: mpsc::UnboundedSender<SyncJob>,
    mode: SyncMode,
) -> LanInboundService {
    let settings_repository = Arc::new(MemorySettingsRepository {
        manual_default_mode: mode,
    });
    let coordinator = Arc::new(SyncJobCoordinator::new(
        Arc::new(RecordingExecutor { jobs }),
        Arc::new(NoopReconciler),
    ));

    LanInboundService::new(
        state,
        settings_repository,
        peer_repository,
        coordinator,
        Arc::new(NoopEvents),
        approval,
    )
}

#[tokio::test]
async fn inbound_pairing_accepts_peer_and_clears_session() {
    let state = Arc::new(LanSyncRuntimeState::new());
    state
        .set_pairing_session(LanPairingSession {
            token: "pair-token".to_string(),
            expires_at_ms: now_ms() + 60_000,
        })
        .await;

    let identity = test_identity(
        test_device_id("11111111-1111-4111-8111-111111111111"),
        "server",
    );
    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: identity.clone(),
        paired_devices: Mutex::new(Vec::new()),
    });
    let approval = Arc::new(StaticApproval {
        accept: true,
        requests: Mutex::new(Vec::new()),
    });
    let (jobs, _job_rx) = mpsc::unbounded_channel();
    let inbound = inbound_service(
        state.clone(),
        peer_repository.clone(),
        approval.clone(),
        jobs,
        SyncMode::Incremental,
    );
    let peer_id = test_device_id("22222222-2222-4222-8222-222222222222");

    let response = inbound
        .complete_pairing(
            "pair-token".to_string(),
            peer_request(peer_id.clone(), "peer"),
        )
        .await
        .expect("complete pairing");

    assert_eq!(response.server_device_id, identity.device_id);
    assert_eq!(response.server_device_name, "server");
    assert_eq!(response.granted_permissions, default_lan_permissions());
    assert!(state.get_pairing_session().await.is_none());

    let devices = peer_repository.load_paired_devices().await.unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].grant.device_id, peer_id);
    assert_eq!(devices[0].grant.device_name, "peer");
    assert_eq!(devices[0].base_url, "https://192.168.1.23:51000");
    assert_eq!(devices[0].spki_sha256, "peer-spki");
    assert_eq!(devices[0].grant.permissions, default_lan_permissions());

    let approval_requests = approval.requests.lock().await;
    assert_eq!(approval_requests.len(), 1);
    assert_eq!(approval_requests[0].peer_device_name, "peer");
    assert_eq!(approval_requests[0].peer_ip, "192.168.1.23");
}

#[tokio::test]
async fn inbound_pairing_rejection_does_not_store_peer() {
    let state = Arc::new(LanSyncRuntimeState::new());
    state
        .set_pairing_session(LanPairingSession {
            token: "pair-token".to_string(),
            expires_at_ms: now_ms() + 60_000,
        })
        .await;

    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        ),
        paired_devices: Mutex::new(Vec::new()),
    });
    let approval = Arc::new(StaticApproval {
        accept: false,
        requests: Mutex::new(Vec::new()),
    });
    let (jobs, _job_rx) = mpsc::unbounded_channel();
    let inbound = inbound_service(
        state.clone(),
        peer_repository.clone(),
        approval,
        jobs,
        SyncMode::Incremental,
    );

    let error = inbound
        .complete_pairing(
            "pair-token".to_string(),
            peer_request(
                test_device_id("22222222-2222-4222-8222-222222222222"),
                "peer",
            ),
        )
        .await
        .expect_err("pairing should be rejected");

    assert!(matches!(
        error,
        DomainError::AuthenticationError(message) if message == PAIRING_REJECTED_MESSAGE
    ));
    assert!(
        peer_repository
            .load_paired_devices()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(state.get_pairing_session().await.is_some());
}

#[tokio::test]
async fn accepted_stale_pairing_request_does_not_clear_new_session() {
    let state = Arc::new(LanSyncRuntimeState::new());
    state
        .set_pairing_session(LanPairingSession {
            token: "old-token".to_string(),
            expires_at_ms: now_ms() + 60_000,
        })
        .await;

    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        ),
        paired_devices: Mutex::new(Vec::new()),
    });
    let (jobs, _job_rx) = mpsc::unbounded_channel();
    let inbound = LanInboundService::new(
        state.clone(),
        Arc::new(MemorySettingsRepository {
            manual_default_mode: SyncMode::Incremental,
        }),
        peer_repository.clone(),
        Arc::new(SyncJobCoordinator::new(
            Arc::new(RecordingExecutor { jobs }),
            Arc::new(NoopReconciler),
        )),
        Arc::new(NoopEvents),
        Arc::new(ReplacingApproval {
            state: state.clone(),
        }),
    );

    let error = inbound
        .complete_pairing(
            "old-token".to_string(),
            peer_request(
                test_device_id("22222222-2222-4222-8222-222222222222"),
                "peer",
            ),
        )
        .await
        .expect_err("stale pairing should fail");

    assert!(matches!(
        error,
        DomainError::AuthenticationError(message) if message == "Invalid pairing token"
    ));
    assert_eq!(
        state.get_pairing_session().await.unwrap().token,
        "new-token"
    );
    assert!(
        peer_repository
            .load_paired_devices()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn inbound_pull_request_starts_remote_request_job() {
    let state = Arc::new(LanSyncRuntimeState::new());
    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        ),
        paired_devices: Mutex::new(Vec::new()),
    });
    let approval = Arc::new(StaticApproval {
        accept: true,
        requests: Mutex::new(Vec::new()),
    });
    let (jobs, mut job_rx) = mpsc::unbounded_channel();
    let inbound = inbound_service(state, peer_repository, approval, jobs, SyncMode::Mirror);
    let peer_id = test_device_id("22222222-2222-4222-8222-222222222222");

    inbound
        .accept_pull_request(peer_id.clone(), SyncOperationOptions::default())
        .await
        .expect("accept pull request");

    let job = tokio::time::timeout(std::time::Duration::from_secs(1), job_rx.recv())
        .await
        .expect("job should execute")
        .expect("job should be recorded");
    assert_eq!(job.execution, SyncExecutionKind::Pull);
    assert_eq!(job.intent, SyncIntent::PullToLocal);
    assert_eq!(
        job.origin,
        SyncOrigin::RemoteRequest {
            peer_id: peer_id.clone()
        }
    );
    match job.endpoint {
        SyncEndpointRef::LanPeer { device_id } => assert_eq!(device_id, peer_id),
        other => panic!("unexpected endpoint: {other:?}"),
    }
    match job.policy {
        ResolvedSyncPolicy::Transfer { mode, .. } => assert_eq!(mode, SyncMode::Mirror),
        other => panic!("unexpected policy: {other:?}"),
    }
}

#[tokio::test]
async fn stop_server_does_not_abort_accepted_inbound_job() {
    let state = Arc::new(LanSyncRuntimeState::new());
    let settings_repository = Arc::new(MemorySettingsRepository {
        manual_default_mode: SyncMode::Incremental,
    });
    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        ),
        paired_devices: Mutex::new(Vec::new()),
    });
    let approval = Arc::new(StaticApproval {
        accept: true,
        requests: Mutex::new(Vec::new()),
    });
    let (started_tx, mut started_rx) = mpsc::unbounded_channel();
    let (release_tx, release_rx) = oneshot::channel();
    let (completed_tx, mut completed_rx) = mpsc::unbounded_channel();
    let coordinator = Arc::new(SyncJobCoordinator::new(
        Arc::new(BlockingExecutor {
            started: started_tx,
            release: Mutex::new(Some(release_rx)),
        }),
        Arc::new(NoopReconciler),
    ));
    let inbound = LanInboundService::new(
        state.clone(),
        settings_repository.clone(),
        peer_repository.clone(),
        coordinator.clone(),
        Arc::new(RecordingEvents {
            completed: completed_tx,
        }),
        approval.clone(),
    );
    let service = LanSyncService::new(
        state,
        settings_repository,
        peer_repository,
        Arc::new(MemoryServerControl),
        Arc::new(NoopAddressDiscovery),
        Arc::new(NoopPairingClient),
        approval,
        coordinator,
    );

    inbound
        .accept_pull_request(
            test_device_id("22222222-2222-4222-8222-222222222222"),
            SyncOperationOptions::default(),
        )
        .await
        .expect("accept pull request");
    tokio::time::timeout(std::time::Duration::from_secs(1), started_rx.recv())
        .await
        .expect("job should start")
        .expect("started job");

    service.stop_server().await.expect("stop server");
    release_tx.send(()).expect("release job");

    let completed = tokio::time::timeout(std::time::Duration::from_secs(1), completed_rx.recv())
        .await
        .expect("job should complete after stop")
        .expect("completion event");
    assert_eq!(completed.files_total, 1);
    assert_eq!(completed.bytes_total, 2);
    assert_eq!(completed.files_deleted, 3);
}
