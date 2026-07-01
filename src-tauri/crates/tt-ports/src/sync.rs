use async_trait::async_trait;
use tt_contracts::sync::{SyncExecutionFailure, SyncExecutionReport, SyncJob, SyncJobEvent};
use tt_domain::errors::DomainError;
use tt_domain::models::tt_sync::{TtSyncIdentity, TtSyncPairedServer};
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse, PairUri};
use ttsync_contract::peer::DeviceId;

#[async_trait]
pub trait DataChangeReconciler: Send + Sync {
    async fn reconcile(&self, reason: &str) -> Result<(), DomainError>;
}

#[async_trait]
pub trait SyncJobExecutor: Send + Sync {
    async fn execute(&self, job: SyncJob) -> Result<SyncExecutionReport, SyncExecutionFailure>;
}

pub trait SyncJobEventPublisher: Send + Sync {
    fn publish_sync_job(&self, event: SyncJobEvent);
}

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
