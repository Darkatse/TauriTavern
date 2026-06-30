use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::application::dto::data_archive_dto::{
    DATA_ARCHIVE_ARTIFACT_AVAILABLE, DATA_ARCHIVE_ARTIFACT_DISPOSED, DATA_ARCHIVE_ARTIFACT_MISSING,
    DATA_ARCHIVE_KIND_EXPORT, DATA_ARCHIVE_STATE_CANCELLED, DATA_ARCHIVE_STATE_COMPLETED,
    DATA_ARCHIVE_STATE_FAILED,
};
use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::domain::errors::DomainError;
use crate::domain::models::data_archive::{
    DataArchiveImportFailure, DataArchiveLocalMutationSummary,
};

use super::*;

struct UnusedExecutor;

impl DataArchiveExecutor for UnusedExecutor {
    fn import_full_data(
        &self,
        _request: ImportArchiveExecutionRequest,
        _report_progress: &mut dyn FnMut(&str, f32, &str),
        _is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveImportExecutionReport, DataArchiveImportFailure> {
        unreachable!()
    }

    fn export_full_data(
        &self,
        _request: ExportArchiveExecutionRequest,
        _report_progress: &mut dyn FnMut(&str, f32, &str),
        _is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveExportExecutionReport, DomainError> {
        unreachable!()
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

struct UnusedFiles;

impl DataArchiveFileGateway for UnusedFiles {
    fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        unreachable!()
    }

    fn prepare_import_archive(
        &self,
        _archive_path: &Path,
        _archive_is_temporary: bool,
        _job_id: &str,
    ) -> Result<ImportArchiveExecutionRequest, DomainError> {
        unreachable!()
    }

    fn prepare_export_archive(
        &self,
        _job_id: &str,
        _protected_paths: &[PathBuf],
    ) -> Result<ExportArchiveExecutionRequest, DomainError> {
        unreachable!()
    }

    fn prepare_user_backup_archive(
        &self,
        _handle: &str,
        _include_secrets: bool,
        _protected_paths: &[PathBuf],
    ) -> Result<UserBackupArchiveTarget, DomainError> {
        unreachable!()
    }

    fn cleanup_directory(&self, _path: &Path) {
        unreachable!()
    }

    fn cleanup_export(&self, _archive_path: &Path) -> Result<(), DomainError> {
        unreachable!()
    }

    fn save_export(&self, _archive_path: &Path, _file_name: &str) -> Result<PathBuf, DomainError> {
        unreachable!()
    }

    fn save_user_backup(
        &self,
        _archive_path: &str,
        _file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        unreachable!()
    }

    fn cleanup_user_backup(&self, _archive_path: &str) -> Result<(), DomainError> {
        unreachable!()
    }
}

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
    export_result: Mutex<Option<Result<String, DomainError>>>,
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

    fn export_ok(file_name: &str) -> Self {
        Self {
            export_result: Mutex::new(Some(Ok(file_name.to_string()))),
            ..Self::default()
        }
    }

    fn export_error(error: DomainError) -> Self {
        Self {
            export_result: Mutex::new(Some(Err(error))),
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
        request: ExportArchiveExecutionRequest,
        _report_progress: &mut dyn FnMut(&str, f32, &str),
        _is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveExportExecutionReport, DomainError> {
        match self
            .export_result
            .lock()
            .expect("lock export result")
            .take()
            .ok_or_else(|| DomainError::InternalError("missing export result".to_string()))?
        {
            Ok(file_name) => Ok(ArchiveExportExecutionReport {
                file_name,
                archive_path: request.output_path,
            }),
            Err(error) => Err(error),
        }
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
    export_protected_paths: Mutex<Vec<Vec<PathBuf>>>,
    save_export_result: Mutex<Option<Result<PathBuf, DomainError>>>,
    cleanup_export_result: Mutex<Option<Result<(), DomainError>>>,
    cleaned_directories: Mutex<Vec<PathBuf>>,
    cleaned_exports: Mutex<Vec<PathBuf>>,
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

    fn with_save_export(saved_path: PathBuf) -> Self {
        Self {
            save_export_result: Mutex::new(Some(Ok(saved_path))),
            ..Self::default()
        }
    }

    fn with_save_export_result(result: Result<PathBuf, DomainError>) -> Self {
        Self {
            save_export_result: Mutex::new(Some(result)),
            ..Self::default()
        }
    }

    fn with_cleanup_export_result(result: Result<(), DomainError>) -> Self {
        Self {
            cleanup_export_result: Mutex::new(Some(result)),
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
        protected_paths: &[PathBuf],
    ) -> Result<ExportArchiveExecutionRequest, DomainError> {
        self.export_protected_paths
            .lock()
            .expect("lock export protected paths")
            .push(protected_paths.to_vec());
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
    }

    fn cleanup_export(&self, archive_path: &Path) -> Result<(), DomainError> {
        self.cleaned_exports
            .lock()
            .expect("lock cleaned exports")
            .push(archive_path.to_path_buf());
        self.cleanup_export_result
            .lock()
            .expect("lock cleanup export result")
            .take()
            .unwrap_or(Ok(()))
    }

    fn save_export(&self, _archive_path: &Path, _file_name: &str) -> Result<PathBuf, DomainError> {
        self.save_export_result
            .lock()
            .expect("lock save export result")
            .take()
            .unwrap_or_else(|| {
                Err(DomainError::InternalError(
                    "missing save export result".to_string(),
                ))
            })
    }

    fn save_user_backup(
        &self,
        _archive_path: &str,
        _file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        unreachable!()
    }

    fn cleanup_user_backup(&self, _archive_path: &str) -> Result<(), DomainError> {
        unreachable!()
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

fn test_runtime_handle() -> tokio::runtime::Handle {
    tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
        static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
        RUNTIME
            .get_or_init(|| tokio::runtime::Runtime::new().expect("create test runtime"))
            .handle()
            .clone()
    })
}

async fn wait_for_job_state(
    service: &DataArchiveService,
    job_id: &str,
    expected_state: &str,
) -> DataArchiveJobStatus {
    for _ in 0..100 {
        let status = service.get_status(job_id).expect("job status");
        if status.state == expected_state {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("job {job_id} did not reach state {expected_state}");
}

async fn wait_until(mut predicate: impl FnMut() -> bool) {
    for _ in 0..100 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("condition was not met");
}

#[test]
fn registries_are_instance_scoped() {
    let job = Arc::new(DataArchiveJobHandle::new("job-1", "export"));
    let first = DataArchiveJobRegistry::new();
    let second = DataArchiveJobRegistry::new();

    first.insert("job-1", job).expect("insert job");

    assert!(first.get("job-1").is_ok());
    assert!(second.get("job-1").is_err());
}

#[test]
fn dropping_registry_requests_job_cancellation() {
    let job = Arc::new(DataArchiveJobHandle::new("job-1", "import"));
    let registry = DataArchiveJobRegistry::new();

    registry.insert("job-1", job.clone()).expect("insert job");
    drop(registry);

    assert!(job.is_cancel_requested());
}

#[test]
fn service_resolves_completed_export_artifact() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
    job.mark_completed_export(
        "tauritavern-data.zip".to_string(),
        PathBuf::from("/tmp/tauritavern-data.zip"),
    )
    .expect("mark completed export");
    jobs.insert("job-1", job).expect("insert job");

    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(UnusedExecutor),
        Arc::new(UnusedFiles),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    let artifact = service
        .completed_export_artifact("job-1")
        .expect("completed export artifact");
    assert_eq!(
        artifact.archive_path,
        PathBuf::from("/tmp/tauritavern-data.zip")
    );
    assert_eq!(artifact.file_name, "tauritavern-data.zip");
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
        jobs,
        test_runtime_handle(),
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

    let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
    let result = status.result.expect("completed import result");
    assert_eq!(result.source_users, vec!["alice"]);
    assert_eq!(result.target_user.as_deref(), Some("alice"));

    wait_until(|| {
        files
            .cleaned_directories
            .lock()
            .expect("lock cleaned directories")
            .contains(&workspace_root)
    })
    .await;
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
        jobs,
        test_runtime_handle(),
        Arc::new(RecordingExecutor::import_ok_without_local_mutation(
            vec!["alice".to_string()],
            "alice",
        )),
        files,
        initializer.clone(),
        reconciler.clone(),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
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
        jobs,
        test_runtime_handle(),
        Arc::new(RecordingExecutor::import_error(
            DomainError::InternalError("boom".to_string()),
            local_applied,
        )),
        files,
        initializer.clone(),
        reconciler.clone(),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
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
        jobs,
        test_runtime_handle(),
        Arc::new(RecordingExecutor::import_error(
            DomainError::Cancelled("cancelled".to_string()),
            local_applied,
        )),
        files,
        initializer.clone(),
        reconciler.clone(),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_CANCELLED).await;
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
        jobs,
        test_runtime_handle(),
        Arc::new(RecordingExecutor::import_error(
            DomainError::InternalError("boom".to_string()),
            local_applied,
        )),
        files,
        Arc::new(RecordingInitializer::default()),
        Arc::new(FailingReconciler),
    );

    let job_id = service
        .start_import(Path::new("/tmp/source.archive"), true)
        .expect("start import");

    let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
    assert_eq!(status.local_applied, Some(local_applied.into()));
    assert_eq!(
        status.reconcile_error.as_deref(),
        Some("failed to refresh runtime caches: Internal error: cache stale")
    );
}

#[tokio::test]
async fn start_export_runs_executor_and_marks_completed() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let output_path = PathBuf::from("/tmp/tauritavern-data.zip");
    let files = Arc::new(RecordingFiles::with_export(ExportArchiveExecutionRequest {
        data_root: PathBuf::from("/tmp/data-root"),
        output_path: output_path.clone(),
        file_name: "tauritavern-data.zip".to_string(),
    }));
    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(RecordingExecutor::export_ok("tauritavern-data.zip")),
        files,
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    let job_id = service.start_export().expect("start export");

    let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
    let result = status.result.expect("completed export result");
    assert_eq!(result.file_name.as_deref(), Some("tauritavern-data.zip"));
    assert_eq!(
        result.archive_path.as_deref(),
        Some(output_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        result.artifact_state.as_deref(),
        Some(DATA_ARCHIVE_ARTIFACT_AVAILABLE)
    );
}

#[test]
fn start_export_uses_runtime_handle_outside_tokio_context() {
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let output_path = PathBuf::from("/tmp/tauritavern-data.zip");
    let files = Arc::new(RecordingFiles::with_export(ExportArchiveExecutionRequest {
        data_root: PathBuf::from("/tmp/data-root"),
        output_path: output_path.clone(),
        file_name: "tauritavern-data.zip".to_string(),
    }));
    let service = DataArchiveService::new(
        jobs,
        runtime.handle().clone(),
        Arc::new(RecordingExecutor::export_ok("tauritavern-data.zip")),
        files,
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    let job_id = service.start_export().expect("start export");

    let status = runtime.block_on(wait_for_job_state(
        &service,
        &job_id,
        DATA_ARCHIVE_STATE_COMPLETED,
    ));
    let result = status.result.expect("completed export result");
    assert_eq!(result.file_name.as_deref(), Some("tauritavern-data.zip"));
    assert_eq!(
        result.archive_path.as_deref(),
        Some(output_path.to_string_lossy().as_ref())
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
        jobs,
        test_runtime_handle(),
        Arc::new(RecordingExecutor::export_error(DomainError::InternalError(
            "boom".to_string(),
        ))),
        files.clone(),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    let job_id = service.start_export().expect("start export");

    let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
    assert_eq!(status.error.as_deref(), Some("Internal error: boom"));
    wait_until(|| {
        files
            .cleaned_exports
            .lock()
            .expect("lock cleaned exports")
            .contains(&output_path)
    })
    .await;
}

#[tokio::test]
async fn start_export_protects_claimed_completed_artifact_from_stale_cleanup() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let existing_job = Arc::new(DataArchiveJobHandle::new(
        "old-job",
        DATA_ARCHIVE_KIND_EXPORT,
    ));
    let existing_path = PathBuf::from("/tmp/old-staged-export.zip");
    existing_job
        .mark_completed_export("tauritavern-data.zip".to_string(), existing_path.clone())
        .expect("mark completed export");
    existing_job
        .claim_export_artifact_path()
        .expect("claim artifact path");
    jobs.insert("old-job", existing_job)
        .expect("insert old job");

    let files = Arc::new(RecordingFiles::with_export(ExportArchiveExecutionRequest {
        data_root: PathBuf::from("/tmp/data-root"),
        output_path: PathBuf::from("/tmp/new-staged-export.zip"),
        file_name: "tauritavern-data.zip".to_string(),
    }));
    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(RecordingExecutor::export_ok("tauritavern-data.zip")),
        files.clone(),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    let _ = service.start_export().expect("start export");

    assert_eq!(
        *files
            .export_protected_paths
            .lock()
            .expect("lock export protected paths"),
        vec![vec![existing_path]]
    );
}

#[tokio::test]
async fn save_export_marks_artifact_disposed_with_saved_path() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
    job.mark_completed_export(
        "tauritavern-data.zip".to_string(),
        PathBuf::from("/tmp/staged-export.zip"),
    )
    .expect("mark completed export");
    jobs.insert("job-1", job).expect("insert job");

    let saved_path = PathBuf::from("/Downloads/tauritavern-data.zip");
    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(UnusedExecutor),
        Arc::new(RecordingFiles::with_save_export(saved_path.clone())),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    assert_eq!(
        service
            .save_export("job-1".to_string())
            .await
            .expect("save export"),
        saved_path
    );

    let status = service.get_status("job-1").expect("job status");
    let result = status.result.expect("export result");
    assert_eq!(
        result.artifact_state.as_deref(),
        Some(DATA_ARCHIVE_ARTIFACT_DISPOSED)
    );
    assert_eq!(
        result.archive_path.as_deref(),
        Some("/tmp/staged-export.zip")
    );
    assert_eq!(
        result.saved_path.as_deref(),
        Some("/Downloads/tauritavern-data.zip")
    );
    let error = match service.completed_export_artifact("job-1") {
        Ok(_) => panic!("disposed artifact should not be reusable"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("already been handled"));
}

#[test]
fn cleanup_export_marks_artifact_disposed_and_is_idempotent() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
    let archive_path = PathBuf::from("/tmp/staged-export.zip");
    job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
        .expect("mark completed export");
    jobs.insert("job-1", job).expect("insert job");

    let files = Arc::new(RecordingFiles::default());
    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(UnusedExecutor),
        files.clone(),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    service.cleanup_export("job-1").expect("cleanup export");
    service
        .cleanup_export("job-1")
        .expect("cleanup export is idempotent");

    assert_eq!(
        *files.cleaned_exports.lock().expect("lock cleaned exports"),
        vec![archive_path.clone(), archive_path]
    );
    let status = service.get_status("job-1").expect("job status");
    let result = status.result.expect("export result");
    assert_eq!(
        result.artifact_state.as_deref(),
        Some(DATA_ARCHIVE_ARTIFACT_DISPOSED)
    );
    assert_eq!(
        result.archive_path.as_deref(),
        Some("/tmp/staged-export.zip")
    );
}

#[test]
fn cleanup_export_marks_missing_when_artifact_is_already_gone() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
    let archive_path = PathBuf::from("/tmp/staged-export.zip");
    job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
        .expect("mark completed export");
    jobs.insert("job-1", job).expect("insert job");

    let files = Arc::new(RecordingFiles::with_cleanup_export_result(Err(
        DomainError::NotFound("gone".to_string()),
    )));
    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(UnusedExecutor),
        files.clone(),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    service.cleanup_export("job-1").expect("cleanup export");
    service
        .cleanup_export("job-1")
        .expect("missing cleanup is idempotent");

    assert_eq!(
        *files.cleaned_exports.lock().expect("lock cleaned exports"),
        vec![archive_path.clone(), archive_path]
    );
    let result = service
        .get_status("job-1")
        .expect("job status")
        .result
        .expect("export result");
    assert_eq!(
        result.artifact_state.as_deref(),
        Some(DATA_ARCHIVE_ARTIFACT_MISSING)
    );
    assert_eq!(
        result.archive_path.as_deref(),
        Some("/tmp/staged-export.zip")
    );
}

#[tokio::test]
async fn save_export_restores_available_artifact_on_save_error() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
    let archive_path = PathBuf::from("/tmp/staged-export.zip");
    job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
        .expect("mark completed export");
    jobs.insert("job-1", job).expect("insert job");

    let files = Arc::new(RecordingFiles::with_save_export_result(Err(
        DomainError::InvalidData("target exists".to_string()),
    )));
    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(UnusedExecutor),
        files.clone(),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    assert!(
        service
            .save_export("job-1".to_string())
            .await
            .expect_err("save should fail")
            .to_string()
            .contains("target exists")
    );
    service.cleanup_export("job-1").expect("cleanup can retry");

    assert_eq!(
        *files.cleaned_exports.lock().expect("lock cleaned exports"),
        vec![archive_path]
    );
}

#[test]
fn finalize_export_delivery_disposes_even_when_cleanup_fails() {
    let jobs = Arc::new(DataArchiveJobRegistry::new());
    let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
    let archive_path = PathBuf::from("/tmp/staged-export.zip");
    job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
        .expect("mark completed export");
    jobs.insert("job-1", job).expect("insert job");

    let files = Arc::new(RecordingFiles::with_cleanup_export_result(Err(
        DomainError::InternalError("permission denied".to_string()),
    )));
    let service = DataArchiveService::new(
        jobs,
        test_runtime_handle(),
        Arc::new(UnusedExecutor),
        files.clone(),
        Arc::new(UnusedInitializer),
        Arc::new(UnusedReconciler),
    );

    assert!(
        service
            .finalize_export_delivery("job-1", Some("content://saved-export".to_string()))
            .expect("finalize delivery")
            .expect("cleanup warning")
            .contains("permission denied")
    );
    service
        .cleanup_export("job-1")
        .expect("disposed cleanup can retry deletion");

    assert_eq!(
        *files.cleaned_exports.lock().expect("lock cleaned exports"),
        vec![archive_path.clone(), archive_path]
    );
    let result = service
        .get_status("job-1")
        .expect("job status")
        .result
        .expect("export result");
    assert_eq!(
        result.artifact_state.as_deref(),
        Some(DATA_ARCHIVE_ARTIFACT_DISPOSED)
    );
    assert_eq!(
        result.archive_path.as_deref(),
        Some("/tmp/staged-export.zip")
    );
    assert_eq!(result.saved_path.as_deref(), Some("content://saved-export"));
}
