use std::fmt::Write;
use std::time::SystemTime;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub path: WorkspacePath,
    pub text: String,
    pub bytes: u64,
    pub sha256: String,
}

impl WorkspaceFile {
    pub fn from_text(path: WorkspacePath, text: String) -> Self {
        Self {
            bytes: text.len() as u64,
            sha256: sha256_hex(text.as_bytes()),
            path,
            text,
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

#[derive(Debug, Clone)]
pub struct WorkspaceAppendResult {
    pub file: WorkspaceFile,
    pub previous_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct WorkspaceMetadata {
    pub kind: WorkspaceEntryKind,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceDirectoryEntry {
    pub path: WorkspacePath,
    pub metadata: WorkspaceMetadata,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub path: WorkspacePath,
    pub kind: WorkspaceEntryKind,
}

#[derive(Debug, Clone)]
pub struct WorkspaceFileList {
    pub entries: Vec<WorkspaceEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceWriteGuard {
    Unchecked,
    MustNotExist,
    MustMatchSha256(String),
}

/// A logical workspace view, including read-only mounted files. Mutations of
/// Run files are coordinated across handles; sequences are not transactions.
#[async_trait]
pub trait WorkspaceFs: Send + Sync {
    /// Read at most the requested size; excess data is an error, not a partial result.
    async fn read_file(
        &self,
        path: &WorkspacePath,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError>;
    /// Stage and replace a regular file. The parent must exist; guard checking
    /// and publication are one serialized operation.
    async fn write_file(
        &self,
        path: &WorkspacePath,
        bytes: &[u8],
        guard: WorkspaceWriteGuard,
    ) -> Result<(), DomainError>;
    /// Native append; an I/O failure can leave a partial append. Do not replay it.
    async fn append_file(&self, path: &WorkspacePath, bytes: &[u8]) -> Result<(), DomainError>;
    /// The before SHA and returned text belong to the same serialized append.
    async fn append_text(
        &self,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError>;
    /// None denotes the logical root, and is only accepted by queries.
    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError>;
    /// List one level, sorted by logical path. Exceeding the limit is an error.
    async fn read_dir(
        &self,
        path: Option<&WorkspacePath>,
        maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError>;
    async fn create_dir(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError>;
    async fn remove(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError>;
    /// Native rename to the exact target path, with no copy/delete fallback.
    async fn rename(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError>;
    /// Copy a regular file to a staged target, then replace. Parent must exist.
    async fn copy_file(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError>;
}

// Text and recursive-list conveniences use the same port, never a second I/O path.
impl dyn WorkspaceFs + '_ {
    pub async fn read_text(&self, path: &WorkspacePath) -> Result<WorkspaceFile, DomainError> {
        let bytes = self.read_file(path, usize::MAX).await?;
        let text = String::from_utf8(bytes)
            .map_err(|_| DomainError::workspace_file_not_text(path.as_str()))?;
        Ok(WorkspaceFile::from_text(path.clone(), text))
    }

    pub async fn write_text(
        &self,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError> {
        self.create_text_parent(path).await?;
        self.write_file(path, text.as_bytes(), guard).await?;
        Ok(WorkspaceFile::from_text(path.clone(), text.to_owned()))
    }

    pub async fn create_text_parent(&self, path: &WorkspacePath) -> Result<(), DomainError> {
        if let Some((parent, _)) = path.as_str().rsplit_once('/') {
            let parent = WorkspacePath::parse(parent)?;
            match self.metadata(Some(&parent)).await {
                Ok(metadata) if metadata.kind == WorkspaceEntryKind::Directory => {}
                Ok(_) => {
                    return Err(DomainError::file_io(
                        "mkdir",
                        parent.as_str(),
                        std::io::Error::from(std::io::ErrorKind::NotADirectory),
                    ));
                }
                Err(DomainError::NotFound(_)) => self.create_dir(&parent, true).await?,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub async fn list_files(
        &self,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        if let Some(path) = path
            && self.metadata(Some(path)).await?.kind == WorkspaceEntryKind::File
        {
            return Ok(WorkspaceFileList {
                entries: vec![WorkspaceEntry {
                    path: path.clone(),
                    kind: WorkspaceEntryKind::File,
                }],
                truncated: false,
            });
        }
        let mut pending = vec![(path.cloned(), 0)];
        let mut entries = Vec::new();
        let mut truncated = false;
        while let Some((directory, level)) = pending.pop() {
            // This existing text-tool interface limits displayed entries. Runtime
            // clients instead use read_dir's explicit hard enumeration limit.
            for entry in self
                .read_dir(directory.as_ref(), usize::MAX)
                .await?
                .into_iter()
                .rev()
            {
                if entries.len() == max_entries {
                    truncated = true;
                    break;
                }
                if entry.metadata.kind == WorkspaceEntryKind::Directory && level < depth {
                    pending.push((Some(entry.path.clone()), level + 1));
                }
                entries.push(WorkspaceEntry {
                    path: entry.path,
                    kind: entry.metadata.kind,
                });
            }
            if truncated {
                break;
            }
        }
        entries.sort_by(|a, b| {
            let a_file = a.kind == WorkspaceEntryKind::File;
            let b_file = b.kind == WorkspaceEntryKind::File;
            a_file
                .cmp(&b_file)
                .then_with(|| a.path.as_str().cmp(b.path.as_str()))
        });
        Ok(WorkspaceFileList { entries, truncated })
    }
}
