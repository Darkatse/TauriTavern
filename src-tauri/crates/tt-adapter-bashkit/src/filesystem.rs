use std::future::Future;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use bashkit::{DirEntry, FileSystem, FileSystemExt, FileType, Metadata};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;
use tt_ports::workspace_fs::{
    WorkspaceEntryKind, WorkspaceFs, WorkspaceMetadata, WorkspaceWriteGuard,
};

const MAX_READ_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 4096;

type Mutation = JoinHandle<Result<(), DomainError>>;

pub(crate) struct WorkspaceFileSystem {
    files: Arc<dyn WorkspaceFs>,
    // A cancelled interpreter drops only its waiter. The task keeps the whole
    // storage operation (including its Run lock and publication) alive.
    mutation: Mutex<Option<Mutation>>,
}

impl WorkspaceFileSystem {
    pub(crate) fn new(files: Arc<dyn WorkspaceFs>) -> Self {
        Self {
            files,
            mutation: Mutex::new(None),
        }
    }

    pub(crate) async fn finish(&self) -> Result<(), DomainError> {
        join_current(&mut *self.mutation.lock().await).await
    }

    async fn mutate<F, Fut>(&self, operation: F) -> bashkit::Result<()>
    where
        F: FnOnce(Arc<dyn WorkspaceFs>) -> Fut + Send,
        Fut: Future<Output = Result<(), DomainError>> + Send + 'static,
    {
        let mut current = self.mutation.lock().await;
        // Joining before constructing the next owned buffer also bounds pending
        // writes when a script repeatedly interrupts a command with `timeout`.
        join_current(&mut current).await.map_err(file_error)?;
        *current = Some(tokio::spawn(operation(self.files.clone())));
        join_current(&mut current).await.map_err(file_error)
    }
}

async fn join_current(current: &mut Option<Mutation>) -> Result<(), DomainError> {
    if let Some(task) = current.as_mut() {
        // Await by reference so dropping this future leaves the handle in place.
        let result = task.await;
        *current = None;
        result.map_err(|error| {
            DomainError::InternalError(format!("Workspace shell file operation failed: {error}"))
        })??;
    }
    Ok(())
}

fn workspace_path(path: &Path) -> bashkit::Result<Option<WorkspacePath>> {
    let normalized = bashkit::normalize_path(path);
    let text = normalized
        .to_str()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "workspace path is not UTF-8"))?;
    if text.contains('\\') {
        return Err(Error::new(ErrorKind::InvalidInput, "use / in workspace paths").into());
    }
    // Bashkit paths are POSIX paths even on Windows; is_absolute() would reject
    // /output there, and no host-path resolution belongs in this bridge.
    let relative = text.trim_start_matches('/');
    if relative.is_empty() {
        Ok(None)
    } else {
        WorkspacePath::parse(relative).map(Some).map_err(file_error)
    }
}

fn entry_path(path: &Path) -> bashkit::Result<WorkspacePath> {
    workspace_path(path)?.ok_or_else(|| {
        Error::new(
            ErrorKind::PermissionDenied,
            "workspace root is not a file entry",
        )
        .into()
    })
}

fn file_error(error: DomainError) -> bashkit::Error {
    let kind = match &error {
        DomainError::NotFound(_) => ErrorKind::NotFound,
        DomainError::InvalidData(_) => ErrorKind::InvalidInput,
        DomainError::WorkspacePathIsDirectory { .. } => ErrorKind::IsADirectory,
        DomainError::FileIo { source, .. } => source.kind(),
        DomainError::Cancelled(_) => ErrorKind::Interrupted,
        _ => ErrorKind::Other,
    };
    Error::new(kind, error).into()
}

fn metadata(value: WorkspaceMetadata) -> Metadata {
    let directory = value.kind == WorkspaceEntryKind::Directory;
    Metadata {
        file_type: if directory {
            FileType::Directory
        } else {
            FileType::File
        },
        size: value.bytes,
        // Display-only VFS modes; the supplied WorkspaceFs owns access policy.
        mode: if directory { 0o755 } else { 0o644 },
        // Bashkit requires timestamps. Epoch represents unavailable metadata,
        // including the synthetic workspace root; never invent a current time.
        modified: value.modified.unwrap_or(SystemTime::UNIX_EPOCH),
        created: value.created.unwrap_or(SystemTime::UNIX_EPOCH),
    }
}

fn unsupported(operation: &str) -> bashkit::Error {
    Error::new(
        ErrorKind::Unsupported,
        format!("{operation} is not supported in the workspace"),
    )
    .into()
}

impl FileSystemExt for WorkspaceFileSystem {}

#[async_trait]
impl FileSystem for WorkspaceFileSystem {
    async fn read_file(&self, path: &Path) -> bashkit::Result<Vec<u8>> {
        self.files
            .read_file(&entry_path(path)?, MAX_READ_BYTES)
            .await
            .map_err(file_error)
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> bashkit::Result<()> {
        let path = entry_path(path)?;
        self.mutate(|files| {
            let content = content.to_vec();
            async move {
                files
                    .write_file(&path, &content, WorkspaceWriteGuard::Unchecked)
                    .await
            }
        })
        .await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> bashkit::Result<()> {
        let path = entry_path(path)?;
        self.mutate(|files| {
            let content = content.to_vec();
            async move { files.append_file(&path, &content).await }
        })
        .await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> bashkit::Result<()> {
        let path = entry_path(path)?;
        self.mutate(|files| async move { files.create_dir(&path, recursive).await })
            .await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> bashkit::Result<()> {
        let path = entry_path(path)?;
        self.mutate(|files| async move { files.remove(&path, recursive).await })
            .await
    }

    async fn stat(&self, path: &Path) -> bashkit::Result<Metadata> {
        self.files
            .metadata(workspace_path(path)?.as_ref())
            .await
            .map(metadata)
            .map_err(file_error)
    }

    async fn read_dir(&self, path: &Path) -> bashkit::Result<Vec<DirEntry>> {
        self.files
            .read_dir(workspace_path(path)?.as_ref(), MAX_DIRECTORY_ENTRIES)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| DirEntry {
                        name: entry.path.as_str().rsplit('/').next().unwrap().to_owned(),
                        metadata: metadata(entry.metadata),
                    })
                    .collect()
            })
            .map_err(file_error)
    }

    async fn exists(&self, path: &Path) -> bashkit::Result<bool> {
        match self.files.metadata(workspace_path(path)?.as_ref()).await {
            Ok(_) => Ok(true),
            Err(DomainError::NotFound(_)) => Ok(false),
            Err(error) => Err(file_error(error)),
        }
    }

    async fn rename(&self, from: &Path, to: &Path) -> bashkit::Result<()> {
        let from = entry_path(from)?;
        let to = entry_path(to)?;
        self.mutate(|files| async move { files.rename(&from, &to).await })
            .await
    }

    async fn copy(&self, from: &Path, to: &Path) -> bashkit::Result<()> {
        let from = entry_path(from)?;
        let to = entry_path(to)?;
        self.mutate(|files| async move { files.copy_file(&from, &to).await })
            .await
    }

    async fn symlink(&self, _target: &Path, _link: &Path) -> bashkit::Result<()> {
        Err(unsupported("symlink"))
    }

    async fn read_link(&self, _path: &Path) -> bashkit::Result<PathBuf> {
        Err(unsupported("readlink"))
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> bashkit::Result<()> {
        Err(unsupported("chmod"))
    }
}
