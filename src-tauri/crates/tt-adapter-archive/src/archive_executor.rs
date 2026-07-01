use tt_domain::errors::DomainError;
use tt_domain::models::data_archive::DataArchiveImportFailure;
use tt_ports::data_archive::{
    ArchiveExportExecutionReport, ArchiveImportExecutionReport, DataArchiveExecutor,
    ExportArchiveExecutionRequest, ImportArchiveExecutionRequest, UserBackupArchiveExecutionRequest,
};

use crate::data_archive::{
    run_export_data_archive, run_export_user_backup_archive, run_import_data_archive,
};

pub struct FileDataArchiveExecutor;

impl DataArchiveExecutor for FileDataArchiveExecutor {
    fn import_full_data(
        &self,
        request: ImportArchiveExecutionRequest,
        report_progress: &mut dyn FnMut(&str, f32, &str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveImportExecutionReport, DataArchiveImportFailure> {
        let result = run_import_data_archive(
            &request.data_root,
            &request.archive_path,
            &request.workspace_root,
            report_progress,
            is_cancelled,
        )?;

        Ok(ArchiveImportExecutionReport {
            source_users: result.source_users,
            target_user: result.target_user,
            local_applied: result.local_applied,
        })
    }

    fn export_full_data(
        &self,
        request: ExportArchiveExecutionRequest,
        report_progress: &mut dyn FnMut(&str, f32, &str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveExportExecutionReport, DomainError> {
        let file_name = request.file_name;
        let result = run_export_data_archive(
            &request.data_root,
            &request.output_path,
            report_progress,
            is_cancelled,
        )?;

        Ok(ArchiveExportExecutionReport {
            file_name,
            archive_path: result.archive_path,
        })
    }

    fn export_user_backup(
        &self,
        request: UserBackupArchiveExecutionRequest,
        report_progress: &mut dyn FnMut(&str, f32, &str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<(), DomainError> {
        run_export_user_backup_archive(
            &request.user_root,
            &request.output_path,
            request.include_secrets,
            report_progress,
            is_cancelled,
        )?;

        Ok(())
    }
}
