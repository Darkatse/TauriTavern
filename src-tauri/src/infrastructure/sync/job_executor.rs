use std::sync::Arc;

use async_trait::async_trait;

use crate::application::services::sync_job_coordinator::SyncJobExecutor;
use crate::domain::errors::DomainError;
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncExecutionKind, SyncJob, SyncJobOutcome,
    SyncJobSummary, SyncOrigin,
};
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;
use crate::infrastructure::sync::lan::notify::request_peer_pull as request_lan_peer_pull;
use crate::infrastructure::sync::lan::pull::pull_from_device as pull_from_lan_device;
use crate::infrastructure::sync::lan::store::LanPeerStore;
use crate::infrastructure::tt_sync::pull::pull_from_server;
use crate::infrastructure::tt_sync::push::push_to_server;
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;

pub struct InfrastructureSyncJobExecutor {
    lan_runtime: Arc<LanSyncRuntime>,
    lan_peer_store: LanPeerStore,
    tt_runtime: Arc<TtSyncRuntime>,
}

impl InfrastructureSyncJobExecutor {
    pub fn new(
        lan_runtime: Arc<LanSyncRuntime>,
        lan_peer_store: LanPeerStore,
        tt_runtime: Arc<TtSyncRuntime>,
    ) -> Self {
        Self {
            lan_runtime,
            lan_peer_store,
            tt_runtime,
        }
    }
}

#[async_trait]
impl SyncJobExecutor for InfrastructureSyncJobExecutor {
    async fn execute(&self, job: SyncJob) -> Result<SyncJobOutcome, DomainError> {
        let SyncJob {
            endpoint,
            execution,
            origin,
            policy,
            ..
        } = job;

        match (endpoint, execution, policy) {
            (
                SyncEndpointRef::LanPeer { device_id },
                SyncExecutionKind::Pull,
                ResolvedSyncPolicy::Transfer { mode, options },
            ) => {
                let completed = pull_from_lan_device(
                    self.lan_runtime.clone(),
                    self.lan_peer_store.clone(),
                    &device_id,
                    mode,
                    options,
                )
                .await?;
                Ok(SyncJobOutcome::Completed {
                    summary: SyncJobSummary::new(
                        completed.files_total,
                        completed.bytes_total,
                        completed.files_deleted,
                    ),
                })
            }
            (
                SyncEndpointRef::LanPeer { device_id },
                SyncExecutionKind::RequestRemotePull,
                ResolvedSyncPolicy::RemotePullRequest { options },
            ) => {
                request_lan_peer_pull(self.lan_peer_store.clone(), &device_id, options).await?;
                Ok(SyncJobOutcome::RemoteRequestAccepted)
            }
            (
                SyncEndpointRef::RemoteServer { server_device_id },
                SyncExecutionKind::Pull,
                ResolvedSyncPolicy::Transfer { mode, options },
            ) => {
                let completed =
                    pull_from_server(self.tt_runtime.clone(), &server_device_id, mode, options)
                        .await?;
                Ok(SyncJobOutcome::Completed {
                    summary: SyncJobSummary::new(
                        completed.files_total,
                        completed.bytes_total,
                        completed.files_deleted,
                    ),
                })
            }
            (
                SyncEndpointRef::RemoteServer { server_device_id },
                SyncExecutionKind::DirectPush,
                ResolvedSyncPolicy::Transfer { mode, options },
            ) => {
                let _origin_guard = matches!(origin, SyncOrigin::Scheduled)
                    .then(|| self.tt_runtime.auto_event_guard());
                let completed =
                    push_to_server(self.tt_runtime.clone(), &server_device_id, mode, options)
                        .await?;
                Ok(SyncJobOutcome::Completed {
                    summary: SyncJobSummary::new(
                        completed.files_total,
                        completed.bytes_total,
                        completed.files_deleted,
                    ),
                })
            }
            _ => Err(DomainError::InvalidData(
                "Sync job endpoint does not support the requested execution".to_string(),
            )),
        }
    }
}
