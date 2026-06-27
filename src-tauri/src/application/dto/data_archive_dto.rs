use serde::Serialize;

pub const DATA_ARCHIVE_STATE_PENDING: &str = "pending";
pub const DATA_ARCHIVE_STATE_RUNNING: &str = "running";
pub const DATA_ARCHIVE_STATE_COMPLETED: &str = "completed";
pub const DATA_ARCHIVE_STATE_FAILED: &str = "failed";
pub const DATA_ARCHIVE_STATE_CANCELLED: &str = "cancelled";

pub const DATA_ARCHIVE_KIND_IMPORT: &str = "import";
pub const DATA_ARCHIVE_KIND_EXPORT: &str = "export";

#[derive(Debug, Clone, Serialize)]
pub struct DataArchiveJobResult {
    pub source_users: Vec<String>,
    pub target_user: Option<String>,
    pub file_name: Option<String>,
    pub archive_path: Option<String>,
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
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserBackupArchiveResult {
    pub file_name: String,
    pub archive_path: String,
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
            }),
            error: None,
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
                    "archive_path": "/tmp/tauritavern-data.zip"
                },
                "error": null,
                "started_at": "2026-06-27T00:00:00Z",
                "finished_at": "2026-06-27T00:00:01Z"
            })
        );
    }
}
