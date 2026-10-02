use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tt_adapter_storage_core::file_system::replace_file_blocking;
use tt_domain::errors::DomainError;
use uuid::Uuid;

use super::{FileWorldInfoRepository, document};

impl FileWorldInfoRepository {
    pub async fn cleanup_orphaned_commit_staging(&self) {
        self.commit_sessions.cleanup_orphans().await;
    }

    pub(super) async fn publish_document(
        &self,
        target: PathBuf,
        json: Vec<u8>,
    ) -> Result<(), DomainError> {
        let directory = self.commit_sessions.directory().to_owned();
        tokio::task::spawn_blocking(move || {
            let document = document::Document::parse(&json).map_err(|error| {
                DomainError::InvalidData(format!("World info {}: {error}", target.display()))
            })?;
            let path = directory.join(format!("{}.publish", Uuid::new_v4()));
            let result = publish(&path, &target, document);
            if let Err(error) = std::fs::remove_file(&path)
                && error.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!(
                    path = %path.display(), %error,
                    "Failed to clean world info publication stage"
                );
            }
            result
        })
        .await
        .map_err(task_error)?
    }
}

/// Every writer creates and atomically publishes the same validated document stage.
pub(super) fn publish(
    path: &Path,
    target: &Path,
    document: document::Document<'_>,
) -> Result<(), DomainError> {
    for parent in [path.parent(), target.parent()].into_iter().flatten() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, "create", error))?;
    }
    let mut file = std::fs::File::options()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| io_error(path, "create", error))?;
    file.write_all(document.bytes())
        .map_err(|error| io_error(path, "write", error))?;
    drop(file);
    replace_file_blocking(path, target)
}

pub(super) fn io_error(path: &Path, operation: &str, error: io::Error) -> DomainError {
    DomainError::InternalError(format!(
        "Failed to {operation} world info file {}: {error}",
        path.display()
    ))
}

pub(super) fn task_error(error: tokio::task::JoinError) -> DomainError {
    DomainError::InternalError(format!("World info commit task failed: {error}"))
}
