use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, Semaphore, mpsc, oneshot};
use ttsync_contract::peer::DeviceId;
use ttsync_contract::sync::{OverwritePolicy, SyncMode};

use super::pairing_link::{default_lan_permissions, device_pubkey_b64url};
use super::ports::LanPairingApprovalRequest;
use super::*;
use crate::services::data_change_reconciler::DataChangeReconciler;
use crate::services::sync_job_coordinator::{SyncJobEventPublisher, SyncJobExecutor};
use crate::services::sync_policy::default_sync_operation_options;
use tt_contracts::sync::{
    LocalAppliedChangeSummary, ResolvedSyncPolicy, SyncEndpointRef, SyncExecutionFailure,
    SyncExecutionKind, SyncExecutionReport, SyncIntent, SyncJob, SyncJobEvent, SyncJobReportResult,
    SyncJobSummary,
};
use tt_domain::models::lan_sync::{
    LanPairCompleteRequest, LanPairCompleteResponse, LanServerSettings, LanSyncIdentity,
    LanSyncPairedDevice, SyncPreferences,
};

struct MemorySettingsRepository {
    preferences: Mutex<SyncPreferences>,
}

impl MemorySettingsRepository {
    fn new(manual_default_mode: SyncMode) -> Self {
        Self {
            preferences: Mutex::new(SyncPreferences {
                manual_default_mode,
                overwrite_policy: OverwritePolicy::Exact,
            }),
        }
    }
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
        Ok(self.preferences.lock().await.clone())
    }

    async fn save_sync_preferences(
        &self,
        preferences: &SyncPreferences,
    ) -> Result<(), DomainError> {
        *self.preferences.lock().await = preferences.clone();
        Ok(())
    }
}

struct MemoryPeerRepository {
    identity: Mutex<LanSyncIdentity>,
    paired_devices: Mutex<Vec<LanSyncPairedDevice>>,
}

#[async_trait]
impl LanPeerRepository for MemoryPeerRepository {
    async fn set_device_name(&self, name: &str) -> Result<(), DomainError> {
        self.identity.lock().await.device_name = name.to_string();
        Ok(())
    }

    async fn update_paired_connection(
        &self,
        device_id: &DeviceId,
        base_url: &str,
        device_name: &str,
        platform: Option<&str>,
    ) -> Result<(), DomainError> {
        let mut peers = self.paired_devices.lock().await;
        let peer = peers
            .iter_mut()
            .find(|peer| &peer.grant.device_id == device_id)
            .expect("paired peer");
        peer.base_url = base_url.to_string();
        peer.grant.device_name = device_name.to_string();
        peer.platform = platform.map(str::to_string);
        Ok(())
    }

    async fn load_or_create_identity(&self) -> Result<LanSyncIdentity, DomainError> {
        Ok(self.identity.lock().await.clone())
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

impl SyncJobEventPublisher for NoopEvents {
    fn publish_sync_job(&self, _event: SyncJobEvent) {}
}

struct RecordingEvents {
    events: mpsc::UnboundedSender<SyncJobEvent>,
}

impl SyncJobEventPublisher for RecordingEvents {
    fn publish_sync_job(&self, event: SyncJobEvent) {
        self.events.send(event).expect("record sync job event");
    }
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

#[derive(Default)]
struct MemoryServerControl(Mutex<Option<LanServerInfo>>);

#[async_trait]
impl LanServerControl for MemoryServerControl {
    async fn start(&self, _port: u16) -> Result<LanServerInfo, DomainError> {
        let info = LanServerInfo {
            port: 51_234,
            spki_sha256: "server-spki".to_string(),
        };
        *self.0.lock().await = Some(info.clone());
        Ok(info)
    }

    async fn stop(&self) -> Result<(), DomainError> {
        *self.0.lock().await = None;
        Ok(())
    }

    async fn running_info(&self) -> Option<LanServerInfo> {
        self.0.lock().await.clone()
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
    async fn probe_device(
        &self,
        _base_url: &str,
        _spki_sha256: Option<&str>,
    ) -> Result<LanDiscoveryAnnouncement, DomainError> {
        Err(DomainError::InternalError("not used".to_string()))
    }
    async fn complete_pairing(
        &self,
        _base_urls: &[String],
        _spki_sha256: &str,
        _expected_device_id: Option<&DeviceId>,
        _local_device: &LanDiscoveryAnnouncement,
        _device_pubkey: &str,
    ) -> Result<(LanPairCompleteResponse, String), DomainError> {
        Err(DomainError::InternalError("not used".to_string()))
    }
}

struct NoopDeviceDiscovery;

#[async_trait]
impl LanDeviceDiscovery for NoopDeviceDiscovery {
    async fn refresh_if_started(&self) -> Result<(), DomainError> {
        Ok(())
    }

    async fn set_device_name(&self, _name: &str) -> Result<(), DomainError> {
        Ok(())
    }
    async fn discover_devices(&self) -> Result<Vec<LanDiscoveredDevice>, DomainError> {
        Ok(Vec::new())
    }
}

fn test_device_id(value: &str) -> DeviceId {
    DeviceId::new(value.to_string()).expect("valid device id")
}

fn test_identity(device_id: DeviceId, device_name: &str) -> LanSyncIdentity {
    LanSyncIdentity {
        device_id,
        device_name: device_name.to_string(),
        platform: "macos".to_string(),
        ed25519_seed: ttsync_core::crypto::random_base64url(32),
    }
}

fn peer_request(device_id: DeviceId, device_name: &str) -> LanPairCompleteRequest {
    let seed = ttsync_core::crypto::random_base64url(32);
    LanPairCompleteRequest {
        device_id,
        device_name: device_name.to_string(),
        device_platform: Some("android".to_string()),
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
    let settings_repository = Arc::new(MemorySettingsRepository::new(mode));
    let coordinator = Arc::new(SyncJobCoordinator::new(
        Arc::new(RecordingExecutor { jobs }),
        Arc::new(NoopReconciler),
        Arc::new(NoopEvents),
        Arc::new(Semaphore::new(1)),
    ));

    LanInboundService::new(
        state,
        settings_repository,
        peer_repository,
        coordinator,
        approval,
    )
}

#[tokio::test]
async fn inbound_pairing_accepts_peer_after_confirmation() {
    let state = Arc::new(LanSyncRuntimeState::new());
    let identity = test_identity(
        test_device_id("11111111-1111-4111-8111-111111111111"),
        "server",
    );
    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: Mutex::new(identity.clone()),
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
        .complete_pairing(peer_request(peer_id.clone(), "peer"))
        .await
        .expect("complete pairing");

    assert_eq!(response.server_device_id, identity.device_id);
    assert_eq!(response.server_device_name, "server");
    assert_eq!(response.server_device_platform.as_deref(), Some("macos"));
    assert_eq!(response.granted_permissions, default_lan_permissions());

    let devices = peer_repository.load_paired_devices().await.unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].grant.device_id, peer_id);
    assert_eq!(devices[0].grant.device_name, "peer");
    assert_eq!(devices[0].platform.as_deref(), Some("android"));
    assert_eq!(devices[0].base_url, "https://192.168.1.23:51000");
    assert_eq!(devices[0].spki_sha256, "peer-spki");
    assert_eq!(devices[0].grant.permissions, default_lan_permissions());

    let approval_requests = approval.requests.lock().await;
    assert_eq!(approval_requests.len(), 1);
    assert_eq!(approval_requests[0].peer_device_name, "peer");
    assert_eq!(approval_requests[0].peer_ip, "192.168.1.23");
}

struct ManualPairingClient {
    device: Mutex<LanDiscoveryAnnouncement>,
    pairings: Mutex<usize>,
}

#[async_trait]
impl LanPairingClient for ManualPairingClient {
    async fn probe_device(
        &self,
        _base_url: &str,
        pin: Option<&str>,
    ) -> Result<LanDiscoveryAnnouncement, DomainError> {
        let device = self.device.lock().await;
        if pin.is_some_and(|pin| pin != device.spki_sha256) {
            return Err(DomainError::AuthenticationError("Wrong pin".to_string()));
        }
        Ok(device.clone())
    }

    async fn complete_pairing(
        &self,
        urls: &[String],
        pin: &str,
        expected: Option<&DeviceId>,
        _local: &LanDiscoveryAnnouncement,
        _pubkey: &str,
    ) -> Result<(LanPairCompleteResponse, String), DomainError> {
        let device = self.device.lock().await;
        assert_eq!(pin, device.spki_sha256);
        assert_eq!(expected, Some(&device.device_id));
        *self.pairings.lock().await += 1;
        let identity = test_identity(device.device_id.clone(), &device.device_name);
        Ok((
            LanPairCompleteResponse {
                server_device_id: identity.device_id,
                server_device_name: identity.device_name,
                server_device_platform: device.platform.clone(),
                server_device_pubkey: device_pubkey_b64url(&identity.ed25519_seed)?,
                granted_permissions: default_lan_permissions(),
            },
            urls[0].clone(),
        ))
    }
}

#[async_trait]
impl LanDeviceDiscovery for ManualPairingClient {
    async fn refresh_if_started(&self) -> Result<(), DomainError> {
        Ok(())
    }

    async fn set_device_name(&self, _name: &str) -> Result<(), DomainError> {
        Ok(())
    }

    async fn discover_devices(&self) -> Result<Vec<LanDiscoveredDevice>, DomainError> {
        let device = self.device.lock().await;
        Ok(vec![LanDiscoveredDevice {
            device_id: device.device_id.clone(),
            device_name: device.device_name.clone(),
            platform: device.platform.clone(),
            base_urls: vec!["https://127.0.0.1:50000".to_string()],
            spki_sha256: device.spki_sha256.clone(),
        }])
    }
}

#[tokio::test]
async fn pairing_starts_the_server_but_relocating_a_paired_device_preserves_trust_and_grants() {
    let local_id = test_device_id("11111111-1111-4111-8111-111111111111");
    let peer_id = test_device_id("22222222-2222-4222-8222-222222222222");
    let peers = Arc::new(MemoryPeerRepository {
        identity: Mutex::new(test_identity(local_id.clone(), "Local")),
        paired_devices: Mutex::new(Vec::new()),
    });
    let client = Arc::new(ManualPairingClient {
        device: Mutex::new(LanDiscoveryAnnouncement {
            device_id: peer_id.clone(),
            device_name: "Phone".to_string(),
            platform: Some("ios".to_string()),
            port: 50_000,
            spki_sha256: "original-pin".to_string(),
        }),
        pairings: Mutex::new(0),
    });
    let server = Arc::new(MemoryServerControl::default());
    let (jobs, _job_rx) = mpsc::unbounded_channel();
    let service = LanSyncService::new(
        Arc::new(LanSyncRuntimeState::new()),
        Arc::new(MemorySettingsRepository::new(SyncMode::Incremental)),
        peers.clone(),
        server.clone(),
        Arc::new(NoopAddressDiscovery),
        client.clone(),
        client.clone(),
        Arc::new(StaticApproval {
            accept: true,
            requests: Mutex::new(Vec::new()),
        }),
        Arc::new(SyncJobCoordinator::new(
            Arc::new(RecordingExecutor { jobs }),
            Arc::new(NoopReconciler),
            Arc::new(NoopEvents),
            Arc::new(Semaphore::new(1)),
        )),
    );

    for invalid in [
        "",
        "127.0.0.1",
        "127.0.0.1:0",
        "0.0.0.0:1234",
        "224.0.0.1:1234",
        "https://user@127.0.0.1:1234",
        "127.0.0.1:1234/path",
    ] {
        assert!(service.connect_address(invalid).await.is_err(), "{invalid}");
    }
    service.connect_address(" 127.0.0.1:50000 ").await.unwrap();
    assert!(server.running_info().await.is_some());
    assert_eq!(*client.pairings.lock().await, 1);
    let mut saved = peers.load_paired_devices().await.unwrap().remove(0);
    assert_eq!(saved.platform.as_deref(), Some("ios"));
    saved.grant.last_sync_ms = Some(42);
    peers.upsert_paired_device(saved.clone()).await.unwrap();
    client.device.lock().await.device_name = "Renamed phone".to_string();
    service.stop_server().await.unwrap();

    service
        .connect_address("https://127.0.0.1:50001/")
        .await
        .unwrap();
    let updated = peers.load_paired_devices().await.unwrap().remove(0);
    assert!(server.running_info().await.is_none());
    assert_eq!(*client.pairings.lock().await, 1);
    assert_eq!(updated.base_url, "https://127.0.0.1:50001");
    assert_eq!(updated.grant.device_name, "Renamed phone");
    assert_eq!(updated.grant.public_key, saved.grant.public_key);
    assert_eq!(updated.grant.permissions, saved.grant.permissions);
    assert_eq!(updated.grant.paired_at_ms, saved.grant.paired_at_ms);
    assert_eq!(updated.grant.last_sync_ms, Some(42));

    client.device.lock().await.spki_sha256 = "replacement-pin".to_string();
    assert!(service.connect_address("127.0.0.1:50002").await.is_err());
    let unchanged = peers.load_paired_devices().await.unwrap().remove(0);
    assert_eq!(unchanged.base_url, updated.base_url);
    assert_eq!(unchanged.spki_sha256, "original-pin");
    client.device.lock().await.device_id = local_id;
    assert!(service.connect_address("127.0.0.1:50003").await.is_err());
    assert_eq!(*client.pairings.lock().await, 1);
    client.device.lock().await.device_id = peer_id.clone();
    service.pair_device(peer_id.as_str()).await.unwrap();
    assert!(server.running_info().await.is_some());
    service.stop_server().await.unwrap();
    let uri = build_pair_uri("https://127.0.0.1:50000", &peer_id, "replacement-pin").unwrap();
    service.request_pairing(&uri).await.unwrap();
    assert!(server.running_info().await.is_some());
    assert_eq!(*client.pairings.lock().await, 3);
}

#[tokio::test]
async fn rejected_pairing_does_not_store_peer() {
    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: Mutex::new(test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        )),
        paired_devices: Mutex::new(Vec::new()),
    });
    let approval = Arc::new(StaticApproval {
        accept: false,
        requests: Mutex::new(Vec::new()),
    });
    let (jobs, _job_rx) = mpsc::unbounded_channel();
    let inbound = inbound_service(
        Arc::new(LanSyncRuntimeState::new()),
        peer_repository.clone(),
        approval,
        jobs,
        SyncMode::Incremental,
    );
    let result = inbound
        .complete_pairing(peer_request(
            test_device_id("22222222-2222-4222-8222-222222222222"),
            "peer",
        ))
        .await;
    assert!(matches!(result, Err(DomainError::AuthenticationError(_))));
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
        identity: Mutex::new(test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        )),
        paired_devices: Mutex::new(Vec::new()),
    });
    let approval = Arc::new(StaticApproval {
        accept: true,
        requests: Mutex::new(Vec::new()),
    });
    let (jobs, mut job_rx) = mpsc::unbounded_channel();
    let inbound = inbound_service(state, peer_repository, approval, jobs, SyncMode::Mirror);
    let peer_id = test_device_id("22222222-2222-4222-8222-222222222222");

    let mut options = default_sync_operation_options();
    options.overwrite_policy = OverwritePolicy::PreferNewer;
    inbound
        .accept_pull_request(peer_id.clone(), options)
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
        ResolvedSyncPolicy::Transfer { mode, options } => {
            assert_eq!(mode, SyncMode::Mirror);
            assert_eq!(options.overwrite_policy, OverwritePolicy::PreferNewer);
        }
        other => panic!("unexpected policy: {other:?}"),
    }
}

#[tokio::test]
async fn stop_server_does_not_abort_accepted_inbound_job() {
    let state = Arc::new(LanSyncRuntimeState::new());
    let settings_repository = Arc::new(MemorySettingsRepository::new(SyncMode::Incremental));
    let peer_repository = Arc::new(MemoryPeerRepository {
        identity: Mutex::new(test_identity(
            test_device_id("11111111-1111-4111-8111-111111111111"),
            "server",
        )),
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
        Arc::new(RecordingEvents {
            events: completed_tx,
        }),
        Arc::new(Semaphore::new(1)),
    ));
    let inbound = LanInboundService::new(
        state.clone(),
        settings_repository.clone(),
        peer_repository.clone(),
        coordinator.clone(),
        approval.clone(),
    );
    let service = LanSyncService::new(
        state,
        settings_repository,
        peer_repository,
        Arc::new(MemoryServerControl::default()),
        Arc::new(NoopAddressDiscovery),
        Arc::new(NoopDeviceDiscovery),
        Arc::new(NoopPairingClient),
        approval,
        coordinator,
    );

    inbound
        .accept_pull_request(
            test_device_id("22222222-2222-4222-8222-222222222222"),
            default_sync_operation_options(),
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
    match completed.result.expect("event result") {
        SyncJobReportResult::Completed { summary } => {
            assert_eq!(summary.files_total, 1);
            assert_eq!(summary.bytes_total, 2);
            assert_eq!(summary.files_deleted, 3);
        }
        other => panic!("unexpected event result: {other:?}"),
    }
}
