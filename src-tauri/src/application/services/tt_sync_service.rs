use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse, PairUri};
use ttsync_contract::peer::DeviceId;
use ttsync_contract::sync::SyncMode;

use crate::application::services::sync_job_coordinator::SyncJobCoordinator;
use crate::domain::errors::DomainError;
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin,
};
use crate::domain::models::tt_sync::{TtSyncIdentity, TtSyncPairedServer};

#[async_trait]
pub trait TtSyncRepository: Send + Sync {
    async fn load_or_create_identity(&self) -> Result<TtSyncIdentity, DomainError>;
    async fn load_paired_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError>;
    async fn upsert_paired_server(&self, server: TtSyncPairedServer) -> Result<(), DomainError>;
    async fn remove_paired_server(&self, server_device_id: &DeviceId) -> Result<(), DomainError>;
}

#[async_trait]
pub trait TtPairingClient: Send + Sync {
    async fn complete_pairing(
        &self,
        pair: &PairUri,
        request: &PairCompleteRequest,
    ) -> Result<PairCompleteResponse, DomainError>;
}

pub struct TtSyncService {
    repository: Arc<dyn TtSyncRepository>,
    pairing_client: Arc<dyn TtPairingClient>,
    coordinator: Arc<SyncJobCoordinator>,
}

impl TtSyncService {
    pub fn new(
        repository: Arc<dyn TtSyncRepository>,
        pairing_client: Arc<dyn TtPairingClient>,
        coordinator: Arc<SyncJobCoordinator>,
    ) -> Self {
        Self {
            repository,
            pairing_client,
            coordinator,
        }
    }

    pub async fn pair(&self, pair_uri: &str) -> Result<TtSyncPairedServer, DomainError> {
        let pair = PairUri::parse_uri_string(pair_uri)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;

        let now_ms = now_ms();
        if now_ms > pair.expires_at_ms {
            return Err(DomainError::InvalidData(format!(
                "Pair URI expired at {} (now {})",
                pair.expires_at_ms, now_ms
            )));
        }

        let identity = self.repository.load_or_create_identity().await?;
        let device_pubkey = ttsync_core::crypto::device_pubkey_b64url(&identity.ed25519_seed)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;

        let request = PairCompleteRequest {
            device_id: identity.device_id,
            device_name: identity.device_name,
            device_pubkey,
        };

        let response = self
            .pairing_client
            .complete_pairing(&pair, &request)
            .await?;

        let server = TtSyncPairedServer {
            server_device_id: response.server_device_id,
            server_device_name: response.server_device_name,
            base_url: pair.url,
            spki_sha256: pair.spki_sha256,
            permissions: response.granted_permissions,
            paired_at_ms: now_ms,
            last_sync_ms: None,
        };

        self.repository.upsert_paired_server(server.clone()).await?;
        Ok(server)
    }

    pub async fn list_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError> {
        self.repository.load_paired_servers().await
    }

    pub async fn remove_server(&self, server_device_id: &str) -> Result<(), DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        self.repository
            .remove_paired_server(&server_device_id)
            .await
    }

    pub async fn pull(
        &self,
        server_device_id: &str,
        mode: SyncMode,
        options: SyncOperationOptions,
    ) -> Result<SyncJobReport, DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        let options = options.validate()?;
        Ok(self
            .run_remote_job(
                server_device_id,
                SyncIntent::PullToLocal,
                SyncOrigin::Manual,
                mode,
                options,
            )
            .await)
    }

    pub async fn push(
        &self,
        server_device_id: &str,
        mode: SyncMode,
        options: SyncOperationOptions,
    ) -> Result<SyncJobReport, DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        let options = options.validate()?;
        let report = self
            .run_remote_job(
                server_device_id,
                SyncIntent::ReplicateLocalToRemote,
                SyncOrigin::Manual,
                mode,
                options,
            )
            .await;
        Ok(report)
    }

    async fn run_remote_job(
        &self,
        server_device_id: DeviceId,
        intent: SyncIntent,
        origin: SyncOrigin,
        mode: SyncMode,
        options: SyncOperationOptions,
    ) -> SyncJobReport {
        self.coordinator
            .run(self.job_request(
                SyncEndpointRef::RemoteServer { server_device_id },
                intent,
                origin,
                mode,
                options,
            ))
            .await
    }

    fn job_request(
        &self,
        endpoint: SyncEndpointRef,
        intent: SyncIntent,
        origin: SyncOrigin,
        mode: SyncMode,
        options: SyncOperationOptions,
    ) -> SyncJobRequest {
        SyncJobRequest {
            endpoint,
            intent,
            origin,
            policy: ResolvedSyncPolicy::Transfer { mode, options },
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
