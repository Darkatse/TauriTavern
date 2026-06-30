mod artifact;
mod job;
mod ports;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::runtime::Handle as TokioRuntimeHandle;
use uuid::Uuid;

use crate::application::dto::data_archive_dto::{DATA_ARCHIVE_KIND_IMPORT, DataArchiveJobStatus};
use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::domain::errors::DomainError;

pub(crate) use job::{DataArchiveJobHandle, DataArchiveJobRegistry};
pub(crate) use ports::{
    ArchiveExportExecutionReport, ArchiveImportExecutionReport, DataArchiveExecutor,
    DataArchiveFileGateway, DataRootInitializer, ExportArchiveExecutionRequest,
    ImportArchiveExecutionRequest, UserBackupArchiveExecutionRequest, UserBackupArchiveTarget,
};

pub struct DataArchiveService {
    jobs: Arc<DataArchiveJobRegistry>,
    runtime: TokioRuntimeHandle,
    executor: Arc<dyn DataArchiveExecutor>,
    files: Arc<dyn DataArchiveFileGateway>,
    data_root_initializer: Arc<dyn DataRootInitializer>,
    reconciler: Arc<dyn DataChangeReconciler>,
}

impl DataArchiveService {
    pub(crate) fn new(
        jobs: Arc<DataArchiveJobRegistry>,
        runtime: TokioRuntimeHandle,
        executor: Arc<dyn DataArchiveExecutor>,
        files: Arc<dyn DataArchiveFileGateway>,
        data_root_initializer: Arc<dyn DataRootInitializer>,
        reconciler: Arc<dyn DataChangeReconciler>,
    ) -> Self {
        Self {
            jobs,
            runtime,
            executor,
            files,
            data_root_initializer,
            reconciler,
        }
    }

    pub fn start_import(
        &self,
        archive_path: &Path,
        archive_is_temporary: bool,
    ) -> Result<String, DomainError> {
        let job_id = Uuid::new_v4().simple().to_string();
        let request =
            self.files
                .prepare_import_archive(archive_path, archive_is_temporary, &job_id)?;
        let workspace_root = request.workspace_root.clone();
        let data_root = request.data_root.clone();
        let job = Arc::new(DataArchiveJobHandle::new(&job_id, DATA_ARCHIVE_KIND_IMPORT));
        if let Err(error) = self.jobs.insert(&job_id, job.clone()) {
            self.files.cleanup_directory(&workspace_root);
            return Err(error);
        }

        let executor = self.executor.clone();
        let files = self.files.clone();
        let data_root_initializer = self.data_root_initializer.clone();
        let reconciler = self.reconciler.clone();
        let runtime = self.runtime.clone();

        runtime.clone().spawn(async move {
            let _ = job.mark_running("starting", "Import job started");

            let blocking_job = job.clone();
            let blocking_result = runtime
                .spawn_blocking(move || {
                    let progress_job = blocking_job.clone();
                    let mut report_progress =
                        move |stage: &str, progress_percent: f32, message: &str| {
                            let _ = progress_job.update_progress(stage, progress_percent, message);
                        };

                    let cancel_job = blocking_job.clone();
                    let is_cancelled = move || cancel_job.is_cancel_requested();

                    executor.import_full_data(request, &mut report_progress, &is_cancelled)
                })
                .await;

            match blocking_result {
                Ok(Ok(result)) => {
                    if result.local_applied.changed() {
                        if let Err(error) = reconcile_import_data_change(
                            &data_root_initializer,
                            &reconciler,
                            &data_root,
                        )
                        .await
                        {
                            let _ = job.mark_failed_after_local_mutation(
                                &format!("Import completed but {}", error),
                                result.local_applied,
                                Some(error),
                            );
                            return;
                        }
                    }
                    let _ = job.mark_completed_import(result.source_users, result.target_user);
                }
                Ok(Err(failure)) => {
                    let cancelled = job.is_cancel_requested() || is_cancelled_error(&failure.error);
                    if failure.local_applied.changed() {
                        let reconcile_error = reconcile_import_data_change(
                            &data_root_initializer,
                            &reconciler,
                            &data_root,
                        )
                        .await
                        .err();

                        if cancelled {
                            let _ = job.mark_cancelled_after_local_mutation(
                                failure.local_applied,
                                reconcile_error,
                            );
                        } else {
                            let _ = job.mark_failed_after_local_mutation(
                                &failure.error.to_string(),
                                failure.local_applied,
                                reconcile_error,
                            );
                        }
                    } else if cancelled {
                        let _ = job.mark_cancelled();
                    } else {
                        let _ = job.mark_failed(&failure.error.to_string());
                    }
                }
                Err(error) => {
                    let _ = job.mark_failed(&format!("Import task join error: {}", error));
                }
            }

            files.cleanup_directory(&workspace_root);
        });

        Ok(job_id)
    }

    pub fn start_export(&self) -> Result<String, DomainError> {
        let job_id = Uuid::new_v4().simple().to_string();
        let protected_paths = self.jobs.protected_export_artifact_paths()?;
        let request = self
            .files
            .prepare_export_archive(&job_id, &protected_paths)?;
        let output_path = request.output_path.clone();
        let job = Arc::new(DataArchiveJobHandle::new_export(
            &job_id,
            output_path.clone(),
        ));
        self.jobs.insert(&job_id, job.clone())?;

        let executor = self.executor.clone();
        let files = self.files.clone();
        let runtime = self.runtime.clone();

        runtime.clone().spawn(async move {
            let _ = job.mark_running("starting", "Export job started");

            let blocking_job = job.clone();
            let blocking_result = runtime
                .spawn_blocking(move || {
                    let progress_job = blocking_job.clone();
                    let mut report_progress =
                        move |stage: &str, progress_percent: f32, message: &str| {
                            let _ = progress_job.update_progress(stage, progress_percent, message);
                        };

                    let cancel_job = blocking_job.clone();
                    let is_cancelled = move || cancel_job.is_cancel_requested();

                    executor.export_full_data(request, &mut report_progress, &is_cancelled)
                })
                .await;

            match blocking_result {
                Ok(Ok(result)) => {
                    let _ = job.mark_completed_export(result.file_name, result.archive_path);
                }
                Ok(Err(error)) => {
                    if job.is_cancel_requested() || is_cancelled_error(&error) {
                        let _ = job.mark_cancelled();
                    } else {
                        let _ = job.mark_failed(&error.to_string());
                    }

                    let _ = files.cleanup_export(&output_path);
                    let _ = job.clear_export_artifact_path();
                }
                Err(error) => {
                    let _ = job.mark_failed(&format!("Export task join error: {}", error));
                    let _ = files.cleanup_export(&output_path);
                    let _ = job.clear_export_artifact_path();
                }
            }
        });

        Ok(job_id)
    }

    pub fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        self.files.prepare_incoming_import_archive_path()
    }

    pub fn get_status(&self, job_id: &str) -> Result<DataArchiveJobStatus, DomainError> {
        self.jobs.get(job_id)?.snapshot()
    }

    pub fn cancel(&self, job_id: &str) -> Result<(), DomainError> {
        self.jobs.get(job_id)?.request_cancel();
        Ok(())
    }
}

fn is_cancelled_error(error: &DomainError) -> bool {
    matches!(error, DomainError::Cancelled(_))
}

async fn reconcile_import_data_change(
    data_root_initializer: &Arc<dyn DataRootInitializer>,
    reconciler: &Arc<dyn DataChangeReconciler>,
    data_root: &Path,
) -> Result<(), String> {
    data_root_initializer
        .initialize_data_root(data_root)
        .await
        .map_err(|error| format!("failed to initialize data directory: {}", error))?;
    reconciler
        .reconcile("import")
        .await
        .map_err(|error| format!("failed to refresh runtime caches: {}", error))?;
    Ok(())
}

async fn run_blocking<T>(
    runtime: TokioRuntimeHandle,
    context: &'static str,
    operation: impl FnOnce() -> Result<T, DomainError> + Send + 'static,
) -> Result<T, DomainError>
where
    T: Send + 'static,
{
    runtime
        .spawn_blocking(operation)
        .await
        .map_err(|error| DomainError::InternalError(format!("{}: {}", context, error)))?
}
