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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LocalAppliedChangeSummary {
    pub files_written: usize,
    pub bytes_written: u64,
    pub files_deleted: usize,
}

impl LocalAppliedChangeSummary {
    pub fn changed(&self) -> bool {
        self.files_written > 0 || self.files_deleted > 0
    }

    pub fn unchanged(&self) -> bool {
        !self.changed()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SyncJobOutcome {
    Completed { summary: SyncJobSummary },
    RemoteRequestAccepted,
}

#[derive(Debug)]
pub struct SyncExecutionReport {
    pub outcome: SyncJobOutcome,
    pub local_applied: LocalAppliedChangeSummary,
}

impl SyncExecutionReport {
    pub fn completed(summary: SyncJobSummary, local_applied: LocalAppliedChangeSummary) -> Self {
        Self {
            outcome: SyncJobOutcome::Completed { summary },
            local_applied,
        }
    }

    pub fn remote_request_accepted() -> Self {
        Self {
            outcome: SyncJobOutcome::RemoteRequestAccepted,
            local_applied: LocalAppliedChangeSummary::default(),
        }
    }
}

#[derive(Debug)]
pub struct SyncExecutionFailure {
    pub error: DomainError,
    pub local_applied: LocalAppliedChangeSummary,
}

impl SyncExecutionFailure {
    pub fn new(error: DomainError, local_applied: LocalAppliedChangeSummary) -> Self {
        Self {
            error,
            local_applied,
        }
    }

    pub fn without_local_mutation(error: DomainError) -> Self {
        Self::new(error, LocalAppliedChangeSummary::default())
    }
}

impl From<DomainError> for SyncExecutionFailure {
    fn from(error: DomainError) -> Self {
        Self::without_local_mutation(error)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncJobFailureKind {
    WithoutLocalMutation,
    AfterPartialLocalMutation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SyncJobReportResult {
    Completed {
        summary: SyncJobSummary,
    },
    RemoteRequestAccepted,
    #[serde(rename = "failed")]
    Failed {
        message: String,
        failure_kind: SyncJobFailureKind,
        #[serde(skip_serializing_if = "LocalAppliedChangeSummary::unchanged")]
        local_applied: LocalAppliedChangeSummary,
        #[serde(skip_serializing_if = "Option::is_none")]
        reconcile_error: Option<String>,
    },
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

    pub fn failed_without_local_mutation(job: SyncJob, message: impl Into<String>) -> Self {
        Self {
            job,
            result: SyncJobReportResult::Failed {
                message: message.into(),
                failure_kind: SyncJobFailureKind::WithoutLocalMutation,
                local_applied: LocalAppliedChangeSummary::default(),
                reconcile_error: None,
            },
        }
    }

    pub fn failed_after_partial_local_mutation(
        job: SyncJob,
        message: impl Into<String>,
        local_applied: LocalAppliedChangeSummary,
        reconcile_error: Option<String>,
    ) -> Self {
        Self {
            job,
            result: SyncJobReportResult::Failed {
                message: message.into(),
                failure_kind: SyncJobFailureKind::AfterPartialLocalMutation,
                local_applied,
                reconcile_error,
            },
        }
    }

    pub fn failure_message(&self) -> Option<&str> {
        match &self.result {
            SyncJobReportResult::Failed { message, .. } => Some(message.as_str()),
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

#[cfg(test)]
mod tests {
    use super::*;

    use ttsync_contract::peer::DeviceId;

    fn job() -> SyncJob {
        SyncJob {
            id: "job-1".to_string(),
            endpoint: SyncEndpointRef::LanPeer {
                device_id: DeviceId::new("11111111-1111-4111-8111-111111111111".to_string())
                    .unwrap(),
            },
            intent: SyncIntent::PullToLocal,
            execution: SyncExecutionKind::Pull,
            origin: SyncOrigin::Manual,
            policy: ResolvedSyncPolicy::RemotePullRequest {
                options: SyncOperationOptions::default(),
            },
        }
    }

    #[test]
    fn failed_report_keeps_legacy_status_with_failure_kind() {
        let report = SyncJobReport::failed_after_partial_local_mutation(
            job(),
            "download failed",
            LocalAppliedChangeSummary {
                files_written: 1,
                bytes_written: 7,
                files_deleted: 0,
            },
            None,
        );
        let value = serde_json::to_value(report).unwrap();

        assert_eq!(value["result"]["status"], "failed");
        assert_eq!(
            value["result"]["failure_kind"],
            "after_partial_local_mutation"
        );
        assert_eq!(value["result"]["local_applied"]["files_written"], 1);
    }

    #[test]
    fn failed_without_local_mutation_omits_empty_local_summary() {
        let report = SyncJobReport::failed_without_local_mutation(job(), "busy");
        let value = serde_json::to_value(report).unwrap();

        assert_eq!(value["result"]["status"], "failed");
        assert_eq!(value["result"]["failure_kind"], "without_local_mutation");
        assert!(value["result"].get("local_applied").is_none());
    }
}
