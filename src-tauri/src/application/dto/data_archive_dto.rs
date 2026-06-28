use serde::Serialize;

use crate::domain::models::data_archive::DataArchiveLocalMutationSummary;

pub const DATA_ARCHIVE_STATE_PENDING: &str = "pending";
pub const DATA_ARCHIVE_STATE_RUNNING: &str = "running";
pub const DATA_ARCHIVE_STATE_COMPLETED: &str = "completed";
pub const DATA_ARCHIVE_STATE_FAILED: &str = "failed";
pub const DATA_ARCHIVE_STATE_CANCELLED: &str = "cancelled";

pub const DATA_ARCHIVE_KIND_IMPORT: &str = "import";
pub const DATA_ARCHIVE_KIND_EXPORT: &str = "export";

pub const DATA_ARCHIVE_ARTIFACT_AVAILABLE: &str = "available";
pub const DATA_ARCHIVE_ARTIFACT_DISPOSED: &str = "disposed";
pub const DATA_ARCHIVE_ARTIFACT_MISSING: &str = "missing";

#[derive(Debug, Clone, Serialize)]
pub struct DataArchiveJobResult {
    pub source_users: Vec<String>,
    pub target_user: Option<String>,
    pub file_name: Option<String>,
    pub archive_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DataArchiveLocalMutationSummaryDto {
    pub files_written: usize,
    pub bytes_written: u64,
    #[serde(skip_serializing_if = "is_false")]
    pub target_changed: bool,
}

impl From<DataArchiveLocalMutationSummary> for DataArchiveLocalMutationSummaryDto {
    fn from(summary: DataArchiveLocalMutationSummary) -> Self {
        Self {
            files_written: summary.files_written,
            bytes_written: summary.bytes_written,
            target_changed: summary.target_changed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DataArchiveJobStatus {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub stage: String,
    pub progress_percent: f32,
    pub message: String,
    pub result: Option<DataArchiveJobResult>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_applied: Option<DataArchiveLocalMutationSummaryDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconcile_error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserBackupArchiveResult {
    pub file_name: String,
    pub archive_path: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn job_status_json_shape_matches_host_abi() {
        let status = DataArchiveJobStatus {
            job_id: "job-1".to_string(),
            kind: DATA_ARCHIVE_KIND_EXPORT.to_string(),
            state: DATA_ARCHIVE_STATE_COMPLETED.to_string(),
            stage: "completed".to_string(),
            progress_percent: 100.0,
            message: "Export completed".to_string(),
            result: Some(DataArchiveJobResult {
                source_users: Vec::new(),
                target_user: None,
                file_name: Some("tauritavern-data.zip".to_string()),
                archive_path: Some("/tmp/tauritavern-data.zip".to_string()),
                artifact_state: Some(DATA_ARCHIVE_ARTIFACT_AVAILABLE.to_string()),
                saved_path: None,
            }),
            error: None,
            local_applied: None,
            reconcile_error: None,
            started_at: "2026-06-27T00:00:00Z".to_string(),
            finished_at: Some("2026-06-27T00:00:01Z".to_string()),
        };

        assert_eq!(
            serde_json::to_value(status).expect("serialize data archive status"),
            json!({
                "job_id": "job-1",
                "kind": "export",
                "state": "completed",
                "stage": "completed",
                "progress_percent": 100.0,
                "message": "Export completed",
                "result": {
                    "source_users": [],
                    "target_user": null,
                    "file_name": "tauritavern-data.zip",
                    "archive_path": "/tmp/tauritavern-data.zip",
                    "artifact_state": "available"
                },
                "error": null,
                "started_at": "2026-06-27T00:00:00Z",
                "finished_at": "2026-06-27T00:00:01Z"
            })
        );
    }

    #[test]
    fn job_status_json_includes_local_mutation_when_present() {
        let status = DataArchiveJobStatus {
            job_id: "job-1".to_string(),
            kind: DATA_ARCHIVE_KIND_IMPORT.to_string(),
            state: DATA_ARCHIVE_STATE_FAILED.to_string(),
            stage: "failed".to_string(),
            progress_percent: 99.0,
            message: "Job failed".to_string(),
            result: None,
            error: Some("failed".to_string()),
            local_applied: Some(
                DataArchiveLocalMutationSummary {
                    files_written: 1,
                    bytes_written: 7,
                    target_changed: true,
                }
                .into(),
            ),
            reconcile_error: Some("cache stale".to_string()),
            started_at: "2026-06-27T00:00:00Z".to_string(),
            finished_at: Some("2026-06-27T00:00:01Z".to_string()),
        };

        assert_eq!(
            serde_json::to_value(status).expect("serialize data archive status"),
            json!({
                "job_id": "job-1",
                "kind": "import",
                "state": "failed",
                "stage": "failed",
                "progress_percent": 99.0,
                "message": "Job failed",
                "result": null,
                "error": "failed",
                "local_applied": {
                    "files_written": 1,
                    "bytes_written": 7,
                    "target_changed": true
                },
                "reconcile_error": "cache stale",
                "started_at": "2026-06-27T00:00:00Z",
                "finished_at": "2026-06-27T00:00:01Z"
            })
        );
    }
}
