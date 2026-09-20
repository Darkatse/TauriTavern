use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

use tt_adapter_storage_core::file_system::unique_temp_path;
use tt_domain::errors::{DomainError, WorkspaceWriteConflictKind};
use tt_domain::models::agent::WorkspacePath;
use tt_ports::workspace_fs::{
    WorkspaceAppendResult, WorkspaceDirectoryEntry, WorkspaceEntryKind, WorkspaceFile, WorkspaceFs,
    WorkspaceMetadata, WorkspaceWriteGuard, sha256_hex,
};

use super::fs_tree::hash_file;

pub(super) struct FileWorkspaceFs {
    pub(super) root: PathBuf,
    pub(super) lock: Arc<RwLock<()>>,
}

impl FileWorkspaceFs {
    // Keep the existing containment boundary. Recursive mkdir resolves the
    // nearest existing parent before creating anything; it does not audit a tree.
    pub(super) async fn resolve(&self, path: &WorkspacePath) -> Result<PathBuf, DomainError> {
        let target = self.root.join(path.as_str());
        let mut parent = target.parent().expect("workspace path has a parent");
        loop {
            match fs::canonicalize(parent).await {
                Ok(canonical) => {
                    if !canonical.starts_with(&self.root) {
                        return Err(DomainError::InvalidData(format!(
                            "Workspace path escapes run directory: {}",
                            path.as_str()
                        )));
                    }
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    parent = parent.parent().ok_or_else(|| {
                        DomainError::InvalidData(format!(
                            "Workspace parent missing: {}",
                            path.as_str()
                        ))
                    })?;
                }
                Err(error) => return Err(DomainError::file_io("resolve", path.as_str(), error)),
            }
        }
        match fs::symlink_metadata(&target).await {
            Ok(metadata) => {
                node_metadata(metadata, path.as_str())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(DomainError::file_io("stat", path.as_str(), error)),
        }
        Ok(target)
    }

    async fn file_target(&self, path: &WorkspacePath) -> Result<PathBuf, DomainError> {
        let target = self.resolve(path).await?;
        match fs::symlink_metadata(&target).await {
            Ok(metadata) if metadata.is_dir() => {
                return Err(DomainError::workspace_path_is_directory(path.as_str()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(DomainError::file_io("stat", path.as_str(), error)),
        }
        Ok(target)
    }

    async fn verify_guard(
        &self,
        target: &Path,
        path: &WorkspacePath,
        guard: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        if guard == WorkspaceWriteGuard::Unchecked {
            return Ok(());
        }
        let current = match hash_file(target).await {
            Ok((sha256, _)) => Some(sha256),
            Err(DomainError::NotFound(_)) => None,
            Err(DomainError::FileIo {
                operation, source, ..
            }) => {
                return Err(DomainError::file_io(operation, path.as_str(), source));
            }
            Err(error) => return Err(error),
        };
        let conflict = match guard {
            WorkspaceWriteGuard::Unchecked => unreachable!(),
            WorkspaceWriteGuard::MustNotExist => current
                .map(|actual_sha256| WorkspaceWriteConflictKind::AlreadyExists { actual_sha256 }),
            WorkspaceWriteGuard::MustMatchSha256(expected_sha256)
                if current.as_ref() != Some(&expected_sha256) =>
            {
                Some(WorkspaceWriteConflictKind::Stale {
                    expected_sha256,
                    actual_sha256: current,
                })
            }
            WorkspaceWriteGuard::MustMatchSha256(_) => None,
        };
        match conflict {
            Some(kind) => Err(DomainError::workspace_write_conflict(path.as_str(), kind)),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl WorkspaceFs for FileWorkspaceFs {
    async fn read_file(
        &self,
        path: &WorkspacePath,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError> {
        let _guard = self.lock.read().await;
        let target = self.file_target(path).await?;
        let file = fs::File::open(&target)
            .await
            .map_err(|e| DomainError::file_io("read", path.as_str(), e))?;
        let mut bytes = Vec::new();
        file.take((maximum_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| DomainError::file_io("read", path.as_str(), e))?;
        if bytes.len() > maximum_bytes {
            return Err(DomainError::InvalidData(format!(
                "Workspace read exceeds {maximum_bytes} bytes: {}",
                path.as_str()
            )));
        }
        Ok(bytes)
    }

    async fn write_file(
        &self,
        path: &WorkspacePath,
        bytes: &[u8],
        guard: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        let _guard = self.lock.write().await;
        let target = self.file_target(path).await?;
        self.verify_guard(&target, path, guard).await?;
        let temp = unique_temp_path(&target);
        let result = async {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .await
                .map_err(|e| DomainError::file_io("write", path.as_str(), e))?;
            file.write_all(bytes)
                .await
                .map_err(|e| DomainError::file_io("write", path.as_str(), e))?;
            file.flush()
                .await
                .map_err(|e| DomainError::file_io("write", path.as_str(), e))?;
            drop(file);
            fs::rename(&temp, &target)
                .await
                .map_err(|e| DomainError::file_io("replace", path.as_str(), e))
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&temp).await;
        }
        result
    }

    async fn append_file(&self, path: &WorkspacePath, bytes: &[u8]) -> Result<(), DomainError> {
        let _guard = self.lock.write().await;
        let target = self.file_target(path).await?;
        append_bytes(&target, path, bytes).await
    }

    async fn append_text(
        &self,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        let _guard = self.lock.write().await;
        let target = self.file_target(path).await?;
        let existing = match fs::read(&target).await {
            Ok(bytes) => Some(
                String::from_utf8(bytes)
                    .map_err(|_| DomainError::workspace_file_not_text(path.as_str()))?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(DomainError::file_io("read", path.as_str(), error)),
        };
        let previous_sha256 = existing.as_ref().map(|value| sha256_hex(value.as_bytes()));
        append_bytes(&target, path, text.as_bytes()).await?;
        let mut updated = existing.unwrap_or_default();
        updated.push_str(text);
        Ok(WorkspaceAppendResult {
            previous_sha256,
            file: WorkspaceFile::from_text(path.clone(), updated),
        })
    }

    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError> {
        let _guard = self.lock.read().await;
        let target = match path {
            Some(path) => self.resolve(path).await?,
            None => self.root.clone(),
        };
        let label = path.map_or("/", WorkspacePath::as_str);
        let metadata = fs::symlink_metadata(target)
            .await
            .map_err(|e| DomainError::file_io("stat", label, e))?;
        node_metadata(metadata, label)
    }

    async fn read_dir(
        &self,
        path: Option<&WorkspacePath>,
        maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        let _guard = self.lock.read().await;
        let target = match path {
            Some(path) => self.resolve(path).await?,
            None => self.root.clone(),
        };
        let label = path.map_or("/", WorkspacePath::as_str);
        let mut reader = fs::read_dir(target)
            .await
            .map_err(|e| DomainError::file_io("list", label, e))?;
        let mut entries = Vec::new();
        while let Some(entry) = reader
            .next_entry()
            .await
            .map_err(|e| DomainError::file_io("list", label, e))?
        {
            if entries.len() == maximum_entries {
                return Err(DomainError::InvalidData(format!(
                    "Workspace directory exceeds {maximum_entries} entries: {label}"
                )));
            }
            let name = entry.file_name().into_string().map_err(|_| {
                DomainError::InvalidData(format!("Workspace filename is not UTF-8: {label}"))
            })?;
            let child = WorkspacePath::parse(match path {
                Some(path) => format!("{}/{name}", path.as_str()),
                None => name,
            })?;
            let metadata = fs::symlink_metadata(entry.path())
                .await
                .map_err(|e| DomainError::file_io("stat", child.as_str(), e))?;
            entries.push(WorkspaceDirectoryEntry {
                metadata: node_metadata(metadata, child.as_str())?,
                path: child,
            });
        }
        entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(entries)
    }

    async fn create_dir(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError> {
        let _guard = self.lock.write().await;
        let target = self.resolve(path).await?;
        let result = if recursive {
            fs::create_dir_all(target).await
        } else {
            fs::create_dir(target).await
        };
        result.map_err(|e| DomainError::file_io("mkdir", path.as_str(), e))
    }

    async fn remove(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError> {
        let _guard = self.lock.write().await;
        let target = self.resolve(path).await?;
        let metadata = fs::symlink_metadata(&target)
            .await
            .map_err(|e| DomainError::file_io("remove", path.as_str(), e))?;
        let result = if metadata.is_dir() {
            if recursive {
                fs::remove_dir_all(target).await
            } else {
                fs::remove_dir(target).await
            }
        } else {
            fs::remove_file(target).await
        };
        result.map_err(|e| DomainError::file_io("remove", path.as_str(), e))
    }

    async fn rename(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        let _guard = self.lock.write().await;
        let from = self.resolve(source).await?;
        let to = self.resolve(target).await?;
        fs::rename(from, to).await.map_err(|e| {
            DomainError::file_io(
                "rename",
                format!("{} -> {}", source.as_str(), target.as_str()),
                e,
            )
        })
    }

    async fn copy_file(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        let _guard = self.lock.write().await;
        let from = self.file_target(source).await?;
        let to = self.file_target(target).await?;
        let temp = unique_temp_path(&to);
        let result = async {
            let mut reader = fs::File::open(from)
                .await
                .map_err(|e| DomainError::file_io("copy", source.as_str(), e))?;
            let mut writer = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .await
                .map_err(|e| DomainError::file_io("copy", target.as_str(), e))?;
            tokio::io::copy(&mut reader, &mut writer)
                .await
                .map_err(|e| DomainError::file_io("copy", target.as_str(), e))?;
            writer
                .flush()
                .await
                .map_err(|e| DomainError::file_io("copy", target.as_str(), e))?;
            drop(writer);
            fs::rename(&temp, &to)
                .await
                .map_err(|e| DomainError::file_io("replace", target.as_str(), e))
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&temp).await;
        }
        result
    }
}

async fn append_bytes(
    target: &Path,
    path: &WorkspacePath,
    bytes: &[u8],
) -> Result<(), DomainError> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(target)
        .await
        .map_err(|e| DomainError::file_io("append", path.as_str(), e))?;
    file.write_all(bytes)
        .await
        .map_err(|e| DomainError::file_io("append", path.as_str(), e))?;
    // Tokio's write can finish before its worker; flush completes the operation
    // while the Run write lock is still held. An error is never auto-retried.
    file.flush()
        .await
        .map_err(|e| DomainError::file_io("append", path.as_str(), e))
}

fn node_metadata(
    metadata: std::fs::Metadata,
    path: &str,
) -> Result<WorkspaceMetadata, DomainError> {
    let kind = if metadata.is_file() {
        WorkspaceEntryKind::File
    } else if metadata.is_dir() {
        WorkspaceEntryKind::Directory
    } else {
        return Err(DomainError::file_io(
            "stat",
            path,
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "workspace supports regular files and directories",
            ),
        ));
    };
    Ok(WorkspaceMetadata {
        kind,
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
    })
}

impl super::FileAgentRepository {
    pub(super) async fn open_run_files(
        &self,
        run_id: &str,
    ) -> Result<FileWorkspaceFs, DomainError> {
        let root = fs::canonicalize(self.load_run_dir(run_id).await?)
            .await
            .map_err(|error| DomainError::file_io("open workspace", run_id, error))?;
        Ok(FileWorkspaceFs {
            root,
            lock: self.workspace_lock(run_id).await,
        })
    }
}
