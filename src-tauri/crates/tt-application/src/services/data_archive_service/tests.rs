use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::dto::data_archive_dto::{
    DATA_ARCHIVE_ARTIFACT_DISPOSED, DATA_ARCHIVE_KIND_EXPORT, DATA_ARCHIVE_STATE_CANCELLED,
    DATA_ARCHIVE_STATE_COMPLETED, DATA_ARCHIVE_STATE_FAILED,
};
use crate::services::data_change_reconciler::DataChangeReconciler;
use tt_domain::errors::DomainError;
use tt_domain::models::data_archive::{DataArchiveImportFailure, DataArchiveLocalMutationSummary};

use super::*;

struct UnusedInitializer;

#[async_trait]
impl DataRootInitializer for UnusedInitializer {
    async fn initialize_data_root(&self, _data_root: &Path) -> Result<(), DomainError> {
        unreachable!()
    }
}

struct UnusedReconciler;

#[async_trait]
impl DataChangeReconciler for UnusedReconciler {
    async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
        unreachable!()
    }
}

#[derive(Default)]
struct RecordingExecutor {
    import_result: Mutex<Option<Result<ArchiveImportExecutionReport, DataArchiveImportFailure>>>,
    export_error: Mutex<Option<DomainError>>,
}

impl RecordingExecutor {
    fn import_ok(source_users: Vec<String>, target_user: &str) -> Self {
        Self {
            import_result: Mutex::new(Some(Ok(ArchiveImportExecutionReport {
                source_users,
                target_user: target_user.to_string(),
                local_applied: import_local_applied(),
            }))),
            ..Self::default()
        }
    }

    fn import_ok_without_local_mutation(source_users: Vec<String>, target_user: &str) -> Self {
        Self {
            import_result: Mutex::new(Some(Ok(ArchiveImportExecutionReport {
                source_users,
                target_user: target_user.to_string(),
                local_applied: DataArchiveLocalMutationSummary::default(),
            }))),
            ..Self::default()
        }
    }

    fn import_error(error: DomainError, local_applied: DataArchiveLocalMutationSummary) -> Self {
        Self {
            import_result: Mutex::new(Some(Err(DataArchiveImportFailure::new(
                error,
                local_applied,
            )))),
            ..Self::default()
        }
    }

    fn export_error(error: DomainError) -> Self {
        Self {
            export_error: Mutex::new(Some(error)),
            ..Self::default()
        }
    }
}

impl DataArchiveExecutor for RecordingExecutor {
    fn import_full_data(
        &self,
        _request: ImportArchiveExecutionRequest,
        _report_progress: &mut dyn FnMut(&str, f32, &str),
        _is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveImportExecutionReport, DataArchiveImportFailure> {
        self.import_result
            .lock()
            .expect("lock import result")
            .take()
            .unwrap_or_else(|| {
                Err(DataArchiveImportFailure::new(
                    DomainError::InternalError("missing import result".to_string()),
                    DataArchiveLocalMutationSummary::default(),
                ))
            })
    }

    fn export_full_data(
        &self,
        _request: ExportArchiveExecutionRequest,
        _report_progress: &mut dyn FnMut(&str, f32, &str),
        _is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveExportExecutionReport, DomainError> {
        Err(self
            .export_error
            .lock()
            .expect("lock export error")
            .take()
            .expect("unexpected export execution"))
    }

    fn export_user_backup(
        &self,
        _request: UserBackupArchiveExecutionRequest,
        _report_progress: &mut dyn FnMut(&str, f32, &str),
        _is_cancelled: &dyn Fn() -> bool,
    ) -> Result<(), DomainError> {
        unreachable!()
    }
}

#[derive(Default)]
struct RecordingFiles {
    import_request: Mutex<Option<ImportArchiveExecutionRequest>>,
    export_request: Mutex<Option<ExportArchiveExecutionRequest>>,
    cleaned_directories: Mutex<Vec<PathBuf>>,
    cleaned_exports: Mutex<Vec<PathBuf>>,
    cleanup_finished: tokio::sync::Notify,
}

impl RecordingFiles {
    fn with_import(request: ImportArchiveExecutionRequest) -> Self {
        Self {
            import_request: Mutex::new(Some(request)),
            ..Self::default()
        }
    }

    fn with_export(request: ExportArchiveExecutionRequest) -> Self {
        Self {
            export_request: Mutex::new(Some(request)),
            ..Self::default()
        }
    }
}

impl DataArchiveFileGateway for RecordingFiles {
    fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        unreachable!()
    }

    fn prepare_import_archive(
        &self,
        _archive_path: &Path,
        _archive_is_temporary: bool,
        _job_id: &str,
    ) -> Result<ImportArchiveExecutionRequest, DomainError> {
        self.import_request
            .lock()
            .expect("lock import request")
            .take()
            .ok_or_else(|| DomainError::InternalError("missing import request".to_string()))
    }

    fn prepare_export_archive(
        &self,
        _job_id: &str,
        _protected_paths: &[PathBuf],
    ) -> Result<ExportArchiveExecutionRequest, DomainError> {
        self.export_request
            .lock()
            .expect("lock export request")
            .take()
            .ok_or_else(|| DomainError::InternalError("missing export request".to_string()))
    }

    fn prepare_user_backup_archive(
        &self,
        _handle: &str,
        _include_secrets: bool,
        _protected_paths: &[PathBuf],
    ) -> Result<UserBackupArchiveTarget, DomainError> {
        unreachable!()
    }

    fn cleanup_directory(&self, path: &Path) {
        self.cleaned_directories
            .lock()
            .expect("lock cleaned directories")
            .push(path.to_path_buf());
        self.cleanup_finished.notify_one();
    }

    fn cleanup_export(&self, archive_path: &Path) -> Result<(), DomainError> {
        self.cleaned_exports
            .lock()
            .expect("lock cleaned exports")
            .push(archive_path.to_path_buf());
        self.cleanup_finished.notify_one();
        Ok(())
    }
}

#[derive(Default)]
struct RecordingInitializer {
    data_roots: Mutex<Vec<PathBuf>>,
}

#[async_trait]
impl DataRootInitializer for RecordingInitializer {
    async fn initialize_data_root(&self, data_root: &Path) -> Result<(), DomainError> {
        self.data_roots
            .lock()
            .expect("lock initialized data roots")
            .push(data_root.to_path_buf());
        Ok(())
    }
}

#[derive(Default)]
struct RecordingReconciler {
    reasons: Mutex<Vec<String>>,
}

#[async_trait]
impl DataChangeReconciler for RecordingReconciler {
    async fn reconcile(&self, reason: &str) -> Result<(), DomainError> {
        self.reasons
            .lock()
            .expect("lock reconcile reasons")
            .push(reason.to_string());
        Ok(())
    }
}

struct FailingReconciler;

#[async_trait]
impl DataChangeReconciler for FailingReconciler {
    async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
        Err(DomainError::InternalError("cache stale".to_string()))
    }
}

fn import_local_applied() -> DataArchiveLocalMutationSummary {
    DataArchiveLocalMutationSummary {
        files_written: 1,
        bytes_written: 7,
        target_changed: true,
    }
}

async fn wait_for_job_cleanup(
    service: &DataArchiveService,
    files: &RecordingFiles,
    job_id: &str,
    expected_state: &str,
) -> DataArchiveJobStatus {
    tokio::time::timeout(Duration::from_secs(5), files.cleanup_finished.notified())
        .await
        .expect("job did not finish cleanup");
    let status = service.get_status(job_id).expect("job status");
    assert_eq!(status.state, expected_state);
    status
}

#[tokio::test]
async fn start_import_runs_executor_initializer_reconciler_and_cleanup() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let data_root = PathBuf::from("/tmp/tauritavern-data-root");
    let workspace_root = PathBuf::from("/tmp/tauritavern-import-workspace");
    let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
        data_root: data_root.clone(),
        archive_path: PathBuf::from("/tmp/import.archive"),
        workspace_root: workspace_root.clone(),
    }));
    let initializer = Arc::new(RecordingInitializer::default());
    let reconciler = Arc::new(RecordingReconciler::default());
    let service = DataArchiveService::new(
        test_database_service(),
        jobs,
        tokio::runtime::Handle::current(),
        Arc::new(RecordingExecutor::import_ok(
            vec!["alice".to_string()],
            "alice",
        )),
        files.clone(),
        initializer.clone(),
        reconciler.clone(),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status =
        wait_for_job_cleanup(&service, &files, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
    let result = status.result.expect("completed import result");
    assert_eq!(result.source_users, vec!["alice"]);
    assert_eq!(result.target_user.as_deref(), Some("alice"));

    assert_eq!(
        *files
            .cleaned_directories
            .lock()
            .expect("cleaned directories"),
        vec![workspace_root]
    );
    assert_eq!(
        *initializer
            .data_roots
            .lock()
            .expect("lock initialized data roots"),
        vec![data_root]
    );
    assert_eq!(
        *reconciler.reasons.lock().expect("lock reconcile reasons"),
        vec!["import".to_string()]
    );
}

#[tokio::test]
async fn start_import_with_no_local_mutation_skips_initializer_and_reconciler() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let data_root = PathBuf::from("/tmp/tauritavern-noop-data-root");
    let workspace_root = PathBuf::from("/tmp/tauritavern-noop-import-workspace");
    let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
        data_root,
        archive_path: PathBuf::from("/tmp/import.archive"),
        workspace_root,
    }));
    let initializer = Arc::new(RecordingInitializer::default());
    let reconciler = Arc::new(RecordingReconciler::default());
    let service = DataArchiveService::new(
        test_database_service(),
        jobs,
        tokio::runtime::Handle::current(),
        Arc::new(RecordingExecutor::import_ok_without_local_mutation(
            vec!["alice".to_string()],
            "alice",
        )),
        files.clone(),
        initializer.clone(),
        reconciler.clone(),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status =
        wait_for_job_cleanup(&service, &files, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
    assert_eq!(status.local_applied, None);
    assert!(
        initializer
            .data_roots
            .lock()
            .expect("lock initialized data roots")
            .is_empty()
    );
    assert!(
        reconciler
            .reasons
            .lock()
            .expect("lock reconcile reasons")
            .is_empty()
    );
}

#[tokio::test]
async fn partial_import_failure_initializes_reconciles_and_reports_local_mutation() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let data_root = PathBuf::from("/tmp/tauritavern-partial-data-root");
    let workspace_root = PathBuf::from("/tmp/tauritavern-partial-import-workspace");
    let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
        data_root: data_root.clone(),
        archive_path: PathBuf::from("/tmp/import.archive"),
        workspace_root,
    }));
    let initializer = Arc::new(RecordingInitializer::default());
    let reconciler = Arc::new(RecordingReconciler::default());
    let local_applied = import_local_applied();
    let service = DataArchiveService::new(
        test_database_service(),
        jobs,
        tokio::runtime::Handle::current(),
        Arc::new(RecordingExecutor::import_error(
            DomainError::InternalError("boom".to_string()),
            local_applied,
        )),
        files.clone(),
        initializer.clone(),
        reconciler.clone(),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status = wait_for_job_cleanup(&service, &files, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
    assert_eq!(status.error.as_deref(), Some("Internal error: boom"));
    assert_eq!(status.local_applied, Some(local_applied.into()));
    assert_eq!(status.reconcile_error, None);
    assert_eq!(
        *initializer
            .data_roots
            .lock()
            .expect("lock initialized data roots"),
        vec![data_root]
    );
    assert_eq!(
        *reconciler.reasons.lock().expect("lock reconcile reasons"),
        vec!["import".to_string()]
    );
}

#[tokio::test]
async fn partial_import_cancel_initializes_reconciles_and_reports_local_mutation() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let data_root = PathBuf::from("/tmp/tauritavern-partial-cancel-data-root");
    let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
        data_root: data_root.clone(),
        archive_path: PathBuf::from("/tmp/import.archive"),
        workspace_root: PathBuf::from("/tmp/tauritavern-partial-cancel-workspace"),
    }));
    let initializer = Arc::new(RecordingInitializer::default());
    let reconciler = Arc::new(RecordingReconciler::default());
    let local_applied = import_local_applied();
    let service = DataArchiveService::new(
        test_database_service(),
        jobs,
        tokio::runtime::Handle::current(),
        Arc::new(RecordingExecutor::import_error(
            DomainError::Cancelled("cancelled".to_string()),
            local_applied,
        )),
        files.clone(),
        initializer.clone(),
        reconciler.clone(),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status =
        wait_for_job_cleanup(&service, &files, &job_id, DATA_ARCHIVE_STATE_CANCELLED).await;
    assert_eq!(status.local_applied, Some(local_applied.into()));
    assert_eq!(
        *initializer
            .data_roots
            .lock()
            .expect("lock initialized data roots"),
        vec![data_root]
    );
    assert_eq!(
        *reconciler.reasons.lock().expect("lock reconcile reasons"),
        vec!["import".to_string()]
    );
}

#[tokio::test]
async fn partial_import_failure_reports_reconcile_error() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let data_root = PathBuf::from("/tmp/tauritavern-partial-reconcile-data-root");
    let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
        data_root,
        archive_path: PathBuf::from("/tmp/import.archive"),
        workspace_root: PathBuf::from("/tmp/tauritavern-partial-reconcile-workspace"),
    }));
    let local_applied = import_local_applied();
    let service = DataArchiveService::new(
        test_database_service(),
        jobs,
        tokio::runtime::Handle::current(),
        Arc::new(RecordingExecutor::import_error(
            DomainError::InternalError("boom".to_string()),
            local_applied,
        )),
        files.clone(),
        Arc::new(RecordingInitializer::default()),
        Arc::new(FailingReconciler),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status = wait_for_job_cleanup(&service, &files, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
    assert_eq!(status.local_applied, Some(local_applied.into()));
    assert_eq!(
        status.reconcile_error.as_deref(),
        Some("failed to refresh runtime caches: Internal error: cache stale")
    );
}

#[tokio::test]
async fn start_export_cleans_partial_archive_on_failure() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let output_path = PathBuf::from("/tmp/partial-tauritavern-data.zip");
    let files = Arc::new(RecordingFiles::with_export(ExportArchiveExecutionRequest {
        data_root: PathBuf::from("/tmp/data-root"),
        output_path: output_path.clone(),
        file_name: "tauritavern-data.zip".to_string(),
    }));
    let service = DataArchiveService::new(
        test_database_service(),
        jobs,
        tokio::runtime::Handle::current(),
        Arc::new(RecordingExecutor::export_error(DomainError::InternalError(
            "boom".to_string(),
        ))),
        files.clone(),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    let job_id = service.start_export().expect("start export");

    let status = wait_for_job_cleanup(&service, &files, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
    assert_eq!(status.error.as_deref(), Some("Internal error: boom"));
    assert_eq!(
        *files.cleaned_exports.lock().expect("cleaned exports"),
        vec![output_path]
    );
}

#[tokio::test]
async fn export_artifact_is_claimed_once_and_disposed_when_delivery_ends() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
    let archive_path = PathBuf::from("/tmp/staged-export.zip");
    job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
        .expect("mark completed export");
    jobs.insert("job-1", job).expect("insert job");
    let service = DataArchiveService::new(
        test_database_service(),
        jobs.clone(),
        tokio::runtime::Handle::current(),
        Arc::new(RecordingExecutor::default()),
        Arc::new(RecordingFiles::default()),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );
    let artifact = service
        .claim_export_artifact("job-1")
        .expect("claim artifact");
    assert_eq!(artifact.archive_path, archive_path);
    assert!(service.claim_export_artifact("job-1").is_err());
    assert_eq!(
        jobs.protected_export_artifact_paths()
            .expect("protected paths"),
        vec![archive_path]
    );

    service
        .finish_export_delivery("job-1")
        .expect("finish delivery");
    assert!(service.claim_export_artifact("job-1").is_err());
    assert!(
        jobs.protected_export_artifact_paths()
            .expect("protected paths")
            .is_empty()
    );
    assert_eq!(
        service
            .get_status("job-1")
            .expect("status")
            .result
            .expect("result")
            .artifact_state
            .as_deref(),
        Some(DATA_ARCHIVE_ARTIFACT_DISPOSED)
    );
}

struct EmptyDatabase;

#[async_trait::async_trait]
impl tt_ports::database::DatabaseBackend for EmptyDatabase {
    async fn execute(
        &self,
        _: tt_contracts::database::DatabaseRequest,
    ) -> Result<tt_contracts::database::DatabaseResponse, DomainError> {
        Ok(tt_contracts::database::DatabaseResponse::Unit)
    }
}

fn test_database_service() -> Arc<crate::services::database_service::DatabaseService> {
    Arc::new(crate::services::database_service::DatabaseService::new(
        Arc::new(EmptyDatabase),
    ))
}
