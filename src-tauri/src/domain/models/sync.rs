use serde::{Deserialize, Serialize};
use ttsync_contract::dataset::DatasetSelection;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::sync::SyncMode;
use ttsync_core::dataset::{ResolvedDatasetPolicy, tauri_tavern_default_selection};

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOperationOptions {
    #[serde(default = "tauri_tavern_default_selection")]
    pub selection: DatasetSelection,
    #[serde(default)]
    pub require_bundle_zstd: bool,
}

impl Default for SyncOperationOptions {
    fn default() -> Self {
        Self {
            selection: tauri_tavern_default_selection(),
            require_bundle_zstd: false,
        }
    }
}

impl SyncOperationOptions {
    pub fn validate(self) -> Result<Self, DomainError> {
        ResolvedDatasetPolicy::from_selection(&self.selection)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        Ok(self)
    }
}

pub fn resolve_sync_options(
    options: Option<SyncOperationOptions>,
) -> Result<SyncOperationOptions, DomainError> {
    options.unwrap_or_default().validate()
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncEndpointRef {
    LanPeer { device_id: DeviceId },
    RemoteServer { server_device_id: DeviceId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncIntent {
    PullToLocal,
    ReplicateLocalToRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncExecutionKind {
    Pull,
    DirectPush,
    RequestRemotePull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncOrigin {
    Manual,
    Scheduled,
    RemoteRequest { peer_id: DeviceId },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolvedSyncPolicy {
    Transfer {
        mode: SyncMode,
        options: SyncOperationOptions,
    },
    RemotePullRequest {
        options: SyncOperationOptions,
    },
}

#[derive(Debug, Clone)]
pub struct SyncJobRequest {
    pub endpoint: SyncEndpointRef,
    pub intent: SyncIntent,
    pub origin: SyncOrigin,
    pub policy: ResolvedSyncPolicy,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncJob {
    pub id: String,
    pub endpoint: SyncEndpointRef,
    pub intent: SyncIntent,
    pub execution: SyncExecutionKind,
    pub origin: SyncOrigin,
    pub policy: ResolvedSyncPolicy,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncJobSummary {
    pub files_total: usize,
    pub bytes_total: u64,
    pub files_deleted: usize,
}

impl SyncJobSummary {
    pub fn new(files_total: usize, bytes_total: u64, files_deleted: usize) -> Self {
        Self {
            files_total,
            bytes_total,
            files_deleted,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SyncJobOutcome {
    Completed { summary: SyncJobSummary },
    RemoteRequestAccepted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SyncJobReportResult {
    Completed { summary: SyncJobSummary },
    RemoteRequestAccepted,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncJobReport {
    pub job: SyncJob,
    pub result: SyncJobReportResult,
}

impl SyncJobReport {
    pub fn from_outcome(job: SyncJob, outcome: SyncJobOutcome) -> Self {
        let result = match outcome {
            SyncJobOutcome::Completed { summary } => SyncJobReportResult::Completed { summary },
            SyncJobOutcome::RemoteRequestAccepted => SyncJobReportResult::RemoteRequestAccepted,
        };
        Self { job, result }
    }

    pub fn failed(job: SyncJob, message: impl Into<String>) -> Self {
        Self {
            job,
            result: SyncJobReportResult::Failed {
                message: message.into(),
            },
        }
    }

    pub fn failure_message(&self) -> Option<&str> {
        match &self.result {
            SyncJobReportResult::Failed { message } => Some(message.as_str()),
            _ => None,
        }
    }

    pub fn completed_summary(&self) -> Option<&SyncJobSummary> {
        match &self.result {
            SyncJobReportResult::Completed { summary } => Some(summary),
            _ => None,
        }
    }
}
