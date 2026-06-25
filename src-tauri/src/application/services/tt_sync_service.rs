use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::Manager;

use ttsync_contract::pair::{PairCompleteRequest, PairUri};
use ttsync_contract::peer::DeviceId;
use ttsync_contract::sync::SyncMode;

use crate::app::AppState;
use crate::application::services::sync_job_coordinator::SyncJobCoordinator;
use crate::domain::errors::DomainError;
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReport, SyncJobRequest,
    SyncOperationOptions, SyncOrigin, resolve_sync_options,
};
use crate::domain::models::tt_sync::{TtSyncDirection, TtSyncErrorEvent, TtSyncPairedServer};
use crate::infrastructure::sync::http_client::{SyncHttpClient, sync_error_to_domain};
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;

pub struct TtSyncService {
    runtime: Arc<TtSyncRuntime>,
    coordinator: Arc<SyncJobCoordinator>,
}

impl TtSyncService {
    pub fn new(runtime: Arc<TtSyncRuntime>, coordinator: Arc<SyncJobCoordinator>) -> Self {
        Self {
            runtime,
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

        let identity = self.runtime.store.load_or_create_identity().await?;
        let device_pubkey = ttsync_core::crypto::device_pubkey_b64url(&identity.ed25519_seed)
            .map_err(sync_error_to_domain)?;

        let request = PairCompleteRequest {
            device_id: identity.device_id,
            device_name: identity.device_name,
            device_pubkey,
        };

        let api = SyncHttpClient::new(pair.url.clone(), pair.spki_sha256.clone())?;
        let response = api.pair_complete(&pair.token, &request).await?;

        let server = TtSyncPairedServer {
            server_device_id: response.server_device_id,
            server_device_name: response.server_device_name,
            base_url: pair.url,
            spki_sha256: pair.spki_sha256,
            permissions: response.granted_permissions,
            paired_at_ms: now_ms,
            last_sync_ms: None,
        };

        self.runtime.upsert_paired_server(server.clone()).await?;
        Ok(server)
    }

    pub async fn list_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError> {
        self.runtime.load_paired_servers().await
    }

    pub async fn remove_server(&self, server_device_id: &str) -> Result<(), DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        self.runtime.remove_paired_server(&server_device_id).await
    }

    pub async fn pull(
        &self,
        server_device_id: &str,
        mode: SyncMode,
        options: Option<SyncOperationOptions>,
    ) -> Result<SyncJobReport, DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        let options = resolve_sync_options(options)?;
        let request = self.job_request(
            SyncEndpointRef::RemoteServer { server_device_id },
            SyncIntent::PullToLocal,
            SyncOrigin::Manual,
            mode,
            options,
        );
        let started = match self.coordinator.try_start(request) {
            Ok(started) => started,
            Err(report) => {
                if let Some(message) = report.failure_message() {
                    self.runtime.emit_error(TtSyncErrorEvent {
                        direction: TtSyncDirection::Pull,
                        message: message.to_string(),
                    })?;
                }
                return Ok(report);
            }
        };

        let executed = started.execute().await;
        let report = if executed.outcome().is_some() {
            match self
                .runtime
                .app_handle()
                .state::<Arc<AppState>>()
                .refresh_after_external_data_change("tt_sync_pull")
                .await
            {
                Ok(()) => executed.finish(),
                Err(error) => {
                    let message = format!(
                        "TT-Sync pull completed but failed to refresh runtime caches: {}",
                        error
                    );
                    let report = executed.finish_with_error(error);
                    self.runtime.emit_error(TtSyncErrorEvent {
                        direction: TtSyncDirection::Pull,
                        message,
                    })?;
                    return Ok(report);
                }
            }
        } else {
            executed.finish()
        };

        self.emit_tt_report(&report, TtSyncDirection::Pull)?;
        Ok(report)
    }

    pub async fn push(
        &self,
        server_device_id: &str,
        mode: SyncMode,
        options: Option<SyncOperationOptions>,
    ) -> Result<SyncJobReport, DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        let options = resolve_sync_options(options)?;
        let report = self
            .run_remote_job(
                server_device_id,
                SyncIntent::ReplicateLocalToRemote,
                SyncOrigin::Manual,
                mode,
                options,
            )
            .await;
        self.emit_tt_report(&report, TtSyncDirection::Push)?;
        Ok(report)
    }

    pub async fn push_for_automation(
        &self,
        server_device_id: &str,
        mode: SyncMode,
        options: Option<SyncOperationOptions>,
    ) -> Result<crate::domain::models::tt_sync::TtSyncCompletedEvent, DomainError> {
        let server_device_id = DeviceId::new(server_device_id.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        let options = resolve_sync_options(options)?;
        let report = self
            .run_remote_job_or_error(
                server_device_id,
                SyncIntent::ReplicateLocalToRemote,
                SyncOrigin::Scheduled,
                mode,
                options,
            )
            .await?;
        let summary = report
            .completed_summary()
            .ok_or_else(|| DomainError::InvalidData(report_failure_message(&report)))?;
        Ok(crate::domain::models::tt_sync::TtSyncCompletedEvent {
            direction: TtSyncDirection::Push,
            files_total: summary.files_total,
            bytes_total: summary.bytes_total,
            files_deleted: summary.files_deleted,
        })
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

    async fn run_remote_job_or_error(
        &self,
        server_device_id: DeviceId,
        intent: SyncIntent,
        origin: SyncOrigin,
        mode: SyncMode,
        options: SyncOperationOptions,
    ) -> Result<SyncJobReport, DomainError> {
        let request = self.job_request(
            SyncEndpointRef::RemoteServer { server_device_id },
            intent,
            origin,
            mode,
            options,
        );
        match self.coordinator.try_start(request) {
            Ok(started) => started.execute().await.finish_or_error(),
            Err(report) => Err(DomainError::InvalidData(report_failure_message(&report))),
        }
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

    fn emit_tt_report(
        &self,
        report: &SyncJobReport,
        direction: TtSyncDirection,
    ) -> Result<(), DomainError> {
        if let Some(summary) = report.completed_summary() {
            self.runtime
                .emit_completed(crate::domain::models::tt_sync::TtSyncCompletedEvent {
                    direction,
                    files_total: summary.files_total,
                    bytes_total: summary.bytes_total,
                    files_deleted: summary.files_deleted,
                })?;
        } else if let Some(message) = report.failure_message() {
            self.runtime.emit_error(TtSyncErrorEvent {
                direction,
                message: message.to_string(),
            })?;
        }
        Ok(())
    }
}

fn report_failure_message(report: &SyncJobReport) -> String {
    report
        .failure_message()
        .unwrap_or("TT-Sync job did not complete")
        .to_string()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
