use async_trait::async_trait;
use chrono::Utc;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::AppHandle;
use tauri::Manager;
use uuid::Uuid;

use crate::infrastructure::paths::RuntimePaths;
use crate::infrastructure::persistence::file_system::DataDirectory;
#[cfg(target_os = "ios")]
use tt_contracts::host::IOS_EXPORT_STAGING_ROOT_NAME;
use tt_domain::errors::DomainError;
use tt_ports::data_archive::{
    DataArchiveFileGateway, DataRootInitializer, ExportArchiveExecutionRequest,
    ImportArchiveExecutionRequest, UserBackupArchiveExecutionRequest, UserBackupArchiveTarget,
};

const EXPORT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) struct TauriDataArchiveFileGateway {
    app_handle: AppHandle,
}

impl TauriDataArchiveFileGateway {
    pub(crate) fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl DataArchiveFileGateway for TauriDataArchiveFileGateway {
    fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        let incoming_dir = self
            .app_handle
            .state::<RuntimePaths>()
            .archive_imports_root
            .join("incoming");
        fs::create_dir_all(&incoming_dir).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create import staging directory {}: {}",
                incoming_dir.display(),
                error
            ))
        })?;

        Ok(incoming_dir.join(format!(
            "tauritavern-import-{}.archive",
            Uuid::new_v4().simple()
        )))
    }

    fn prepare_import_archive(
        &self,
        archive_path: &Path,
        archive_is_temporary: bool,
        job_id: &str,
    ) -> Result<ImportArchiveExecutionRequest, DomainError> {
        if !archive_path.is_file() {
            return Err(DomainError::InvalidData(format!(
                "Archive file does not exist: {}",
                archive_path.display()
            )));
        }

        let runtime_paths = self.app_handle.state::<RuntimePaths>();
        let imports_root = runtime_paths.archive_imports_root.clone();
        fs::create_dir_all(&imports_root).map_err(|error| {
            DomainError::InternalError(format!("Failed to create job root: {}", error))
        })?;

        let workspace_root = imports_root.join(job_id);
        fs::create_dir_all(&workspace_root).map_err(|error| {
            DomainError::InternalError(format!("Failed to create job workspace: {}", error))
        })?;

        let archive_path = match prepare_import_archive_path(
            archive_path,
            &workspace_root,
            archive_is_temporary,
        ) {
            Ok(archive_path) => archive_path,
            Err(error) => {
                cleanup_directory(&workspace_root);
                return Err(error);
            }
        };

        Ok(ImportArchiveExecutionRequest {
            data_root: runtime_paths.data_root.clone(),
            archive_path,
            workspace_root,
        })
    }

    fn prepare_export_archive(
        &self,
        job_id: &str,
        protected_paths: &[PathBuf],
    ) -> Result<ExportArchiveExecutionRequest, DomainError> {
        let runtime_paths = self.app_handle.state::<RuntimePaths>();
        let export_root = runtime_paths.archive_exports_root.clone();
        fs::create_dir_all(&export_root).map_err(|error| {
            DomainError::InternalError(format!("Failed to create export directory: {}", error))
        })?;
        cleanup_stale_exports(&export_root, protected_paths);
        let file_name = default_export_file_name();

        Ok(ExportArchiveExecutionRequest {
            data_root: runtime_paths.data_root.clone(),
            output_path: export_root.join(full_export_staging_file_name(job_id)),
            file_name,
        })
    }

    fn prepare_user_backup_archive(
        &self,
        handle: &str,
        include_secrets: bool,
        protected_paths: &[PathBuf],
    ) -> Result<UserBackupArchiveTarget, DomainError> {
        let runtime_paths = self.app_handle.state::<RuntimePaths>();
        let export_root = resolve_user_backup_export_root(&self.app_handle, &runtime_paths)?;
        fs::create_dir_all(&export_root).map_err(|error| {
            DomainError::InternalError(format!("Failed to create export directory: {}", error))
        })?;
        cleanup_stale_exports(&export_root, protected_paths);

        let (handle, user_root) = resolve_user_backup_root(&runtime_paths.data_root, handle)?;
        let file_name = default_user_backup_file_name(&handle);
        let output_path = export_root.join(format!(
            ".user-backup-{}-{}",
            Uuid::new_v4().simple(),
            file_name
        ));

        Ok(UserBackupArchiveTarget {
            file_name,
            request: UserBackupArchiveExecutionRequest {
                user_root,
                output_path,
                include_secrets,
            },
        })
    }

    fn cleanup_directory(&self, path: &Path) {
        cleanup_directory(path);
    }

    fn cleanup_export(&self, archive_path: &Path) -> Result<(), DomainError> {
        fs::remove_file(archive_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "Export archive file not found: {}",
                    archive_path.display()
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to cleanup export archive {}: {}",
                    archive_path.display(),
                    error
                ))
            }
        })
    }

    fn save_export(&self, archive_path: &Path, file_name: &str) -> Result<PathBuf, DomainError> {
        save_staged_archive_to_downloads(&self.app_handle, archive_path, file_name)
    }

    fn save_user_backup(
        &self,
        archive_path: &str,
        file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        let source_path = resolve_staged_user_backup_archive_path(&self.app_handle, archive_path)?;
        save_staged_archive_to_downloads(&self.app_handle, &source_path, file_name)
    }

    fn cleanup_user_backup(&self, archive_path: &str) -> Result<(), DomainError> {
        let source_path = resolve_staged_user_backup_archive_path(&self.app_handle, archive_path)?;
        remove_file_if_exists(&source_path, "cleanup user backup archive");
        Ok(())
    }
}

pub(crate) struct DataDirectoryDataRootInitializer;

#[async_trait]
impl DataRootInitializer for DataDirectoryDataRootInitializer {
    async fn initialize_data_root(&self, data_root: &Path) -> Result<(), DomainError> {
        DataDirectory::new(data_root.to_path_buf())
            .initialize()
            .await
    }
}

fn save_staged_archive_to_downloads(
    app_handle: &AppHandle,
    source_path: &Path,
    file_name: &str,
) -> Result<PathBuf, DomainError> {
    if cfg!(target_os = "android") {
        return Err(DomainError::InternalError(
            "Android archive exports must use the native document save bridge".to_string(),
        ));
    }

    if !source_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "Export archive file not found: {}",
            source_path.display()
        )));
    }

    let file_name = validate_archive_file_name(file_name)?;
    let download_dir = app_handle.path().download_dir().map_err(|error| {
        DomainError::InternalError(format!("Failed to resolve downloads directory: {}", error))
    })?;
    fs::create_dir_all(&download_dir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create downloads directory {}: {}",
            download_dir.display(),
            error
        ))
    })?;

    let target_path = download_dir.join(&file_name);
    if target_path.exists() {
        return Err(DomainError::InvalidData(format!(
            "Export target already exists: {}",
            target_path.display()
        )));
    }

    if fs::rename(source_path, &target_path).is_ok() {
        return Ok(target_path);
    }

    if let Err(error) = fs::copy(source_path, &target_path) {
        remove_file_if_exists(&target_path, "cleanup partial export save");
        return Err(DomainError::InternalError(format!(
            "Failed to save export archive {} to {}: {}",
            source_path.display(),
            target_path.display(),
            error
        )));
    }

    if let Err(error) = fs::remove_file(source_path) {
        remove_file_if_exists(&target_path, "cleanup partial export save");
        return Err(DomainError::InternalError(format!(
            "Failed to remove staged export archive {}: {}",
            source_path.display(),
            error
        )));
    }

    Ok(target_path)
}

fn validate_archive_file_name(file_name: &str) -> Result<String, DomainError> {
    let file_name = file_name.trim();
    if file_name.is_empty() {
        return Err(DomainError::InvalidData(
            "Export archive filename is required".to_string(),
        ));
    }

    if file_name.contains('/') || file_name.contains('\\') {
        return Err(DomainError::InvalidData(format!(
            "Invalid export archive filename: {}",
            file_name
        )));
    }

    let mut components = Path::new(file_name).components();
    let component = components.next();
    if !matches!(component, Some(Component::Normal(_))) || components.next().is_some() {
        return Err(DomainError::InvalidData(format!(
            "Invalid export archive filename: {}",
            file_name
        )));
    }

    Ok(file_name.to_string())
}

#[cfg(target_os = "ios")]
fn candidate_user_backup_export_roots(
    app_handle: &AppHandle,
    _runtime_paths: &RuntimePaths,
) -> Result<Vec<PathBuf>, DomainError> {
    let path_resolver = app_handle.path();
    let mut roots = Vec::new();

    if let Ok(cache_dir) = path_resolver.app_cache_dir() {
        roots.push(
            cache_dir
                .join(IOS_EXPORT_STAGING_ROOT_NAME)
                .join("user-backups"),
        );
    }

    if let Ok(temp_dir) = path_resolver.temp_dir() {
        roots.push(
            temp_dir
                .join(IOS_EXPORT_STAGING_ROOT_NAME)
                .join("user-backups"),
        );
    }

    if roots.is_empty() {
        return Err(DomainError::InternalError(
            "No writable iOS user backup staging directory is available".to_string(),
        ));
    }

    Ok(roots)
}

#[cfg(not(target_os = "ios"))]
fn candidate_user_backup_export_roots(
    _app_handle: &AppHandle,
    runtime_paths: &RuntimePaths,
) -> Result<Vec<PathBuf>, DomainError> {
    Ok(vec![runtime_paths.archive_exports_root.clone()])
}

fn resolve_user_backup_export_root(
    app_handle: &AppHandle,
    runtime_paths: &RuntimePaths,
) -> Result<PathBuf, DomainError> {
    let roots = candidate_user_backup_export_roots(app_handle, runtime_paths)?;
    roots.into_iter().next().ok_or_else(|| {
        DomainError::InternalError(
            "No writable user backup staging directory is available".to_string(),
        )
    })
}

fn resolve_staged_user_backup_archive_path(
    app_handle: &AppHandle,
    archive_path: &str,
) -> Result<PathBuf, DomainError> {
    let archive_path = archive_path.trim();
    if archive_path.is_empty() {
        return Err(DomainError::InvalidData(
            "User backup archive path is required".to_string(),
        ));
    }

    let requested_path = PathBuf::from(archive_path);
    if !requested_path.is_absolute() {
        return Err(DomainError::InvalidData(
            "User backup archive path must be absolute".to_string(),
        ));
    }

    let canonical_path = fs::canonicalize(&requested_path).map_err(|_| {
        DomainError::NotFound(format!(
            "User backup archive file not found: {}",
            requested_path.display()
        ))
    })?;
    if !canonical_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "User backup archive file not found: {}",
            canonical_path.display()
        )));
    }

    let runtime_paths = app_handle.state::<RuntimePaths>();
    let roots = candidate_user_backup_export_roots(app_handle, &runtime_paths)?;
    let mut canonical_roots = Vec::new();
    for root in roots {
        match fs::canonicalize(&root) {
            Ok(root) => canonical_roots.push(root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to resolve user backup staging directory {}: {}",
                    root.display(),
                    error
                )));
            }
        }
    }

    if canonical_roots
        .iter()
        .any(|root| canonical_path.starts_with(root))
    {
        return Ok(canonical_path);
    }

    Err(DomainError::InvalidData(format!(
        "User backup archive path is outside the staging directory: {}",
        requested_path.display()
    )))
}

fn resolve_user_backup_root(
    data_root: &Path,
    handle: &str,
) -> Result<(String, PathBuf), DomainError> {
    let handle = handle.trim();
    if handle.is_empty() {
        return Err(DomainError::InvalidData(
            "User handle is required for backup".to_string(),
        ));
    }

    if handle.contains('/') || handle.contains('\\') {
        return Err(DomainError::InvalidData(format!(
            "Invalid user handle for backup: {}",
            handle
        )));
    }

    let mut components = Path::new(handle).components();
    let component = components.next();
    if !matches!(component, Some(Component::Normal(_))) || components.next().is_some() {
        return Err(DomainError::InvalidData(format!(
            "Invalid user handle for backup: {}",
            handle
        )));
    }

    let user_root = data_root.join(handle);
    if !user_root.is_dir() {
        return Err(DomainError::NotFound(format!(
            "User directory not found: {}",
            handle
        )));
    }

    Ok((handle.to_string(), user_root))
}

fn default_export_file_name() -> String {
    format!(
        "tauritavern-data-{}.zip",
        Utc::now().format("%Y%m%d-%H%M%S")
    )
}

fn default_user_backup_file_name(handle: &str) -> String {
    format!("{}-{}.zip", handle, Utc::now().format("%Y%m%d-%H%M%S"))
}

fn full_export_staging_file_name(job_id: &str) -> String {
    format!("export-{}.zip", job_id)
}

fn prepare_import_archive_path(
    source_archive_path: &Path,
    workspace_root: &Path,
    archive_is_temporary: bool,
) -> Result<PathBuf, DomainError> {
    if !archive_is_temporary {
        return Ok(source_archive_path.to_path_buf());
    }

    let staged_archive_path = workspace_root.join("import.archive");
    if fs::rename(source_archive_path, &staged_archive_path).is_ok() {
        return Ok(staged_archive_path);
    }

    fs::copy(source_archive_path, &staged_archive_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to copy temporary archive to job workspace: {}",
            error
        ))
    })?;

    if let Err(remove_error) = fs::remove_file(source_archive_path) {
        if remove_error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Failed to remove temporary source archive {}: {}",
                source_archive_path.display(),
                remove_error
            );
        }
    }

    Ok(staged_archive_path)
}

fn cleanup_directory(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("Failed to cleanup directory {}: {}", path.display(), error);
        }
    }
}

fn remove_file_if_exists(path: &Path, operation: &str) {
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("Failed to {} {}: {}", operation, path.display(), error);
        }
    }
}

fn cleanup_stale_exports(export_root: &Path, protected_paths: &[PathBuf]) {
    let Ok(entries) = fs::read_dir(export_root) else {
        return;
    };

    let now = SystemTime::now();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if protected_paths.iter().any(|protected| protected == &path) {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        let Ok(modified) = metadata.modified() else {
            continue;
        };

        let Ok(age) = now.duration_since(modified) else {
            continue;
        };

        if age <= EXPORT_RETENTION {
            continue;
        }

        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to remove stale export {}: {}",
                    path.display(),
                    error
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use filetime::{FileTime, set_file_mtime};
    use uuid::Uuid;

    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-data-archive-adapters-{}-{}",
            name,
            Uuid::new_v4().simple()
        ))
    }

    fn make_stale(path: &Path) {
        let stale_time = FileTime::from_system_time(
            SystemTime::now() - EXPORT_RETENTION - Duration::from_secs(1),
        );
        set_file_mtime(path, stale_time).expect("set file mtime");
    }

    #[test]
    fn cleanup_stale_exports_keeps_protected_active_artifact() {
        let root = temp_root("protected-export");
        fs::create_dir_all(&root).expect("create export root");
        let protected = root.join("export-active.zip");
        let stale = root.join("export-stale.zip");
        fs::write(&protected, b"active").expect("write protected export");
        fs::write(&stale, b"stale").expect("write stale export");
        make_stale(&protected);
        make_stale(&stale);

        cleanup_stale_exports(&root, &[protected.clone()]);

        assert!(protected.is_file());
        assert!(!stale.exists());

        fs::remove_dir_all(root).expect("cleanup temp root");
    }
}
