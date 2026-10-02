mod json;
#[cfg(test)]
mod tests;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tt_contracts::byte_commit::CommitBegin;
use tt_contracts::extension_store::{EntryKind, WriteOperation};
use tt_domain::errors::DomainError;
use tt_ports::byte_reader::ByteReader;
use tt_ports::repositories::extension_store_repository::ExtensionStoreRepository;

use crate::commit_stage::{CommitSession, CommitSessions, PageGeneration};
use crate::file_system::replace_file_blocking;

pub struct FileExtensionStoreRepository {
    base_dir: PathBuf,
    commits: CommitSessions<WriteTarget>,
    // Serialize store mutations; use finer locks only if contention warrants it.
    mutation: Arc<Mutex<()>>,
}

struct WriteTarget {
    path: PathBuf,
    operation: WriteOperation,
}

struct EntryReader {
    file: fs::File,
    path: PathBuf,
}

#[async_trait]
impl ByteReader for EntryReader {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, DomainError> {
        self.file
            .read(buffer)
            .await
            .map_err(|error| io_error("read", &self.path, error))
    }
}

impl FileExtensionStoreRepository {
    pub fn new(base_dir: PathBuf, staging_dir: PathBuf, page: Arc<PageGeneration>) -> Self {
        Self {
            base_dir,
            commits: CommitSessions::new(staging_dir, page),
            mutation: Arc::new(Mutex::new(())),
        }
    }

    pub async fn cleanup_orphaned_commit_staging(&self) {
        self.commits.cleanup_orphans().await;
    }

    fn namespace_root(&self, namespace: &str) -> Result<PathBuf, DomainError> {
        Ok(self
            .base_dir
            .join(validate_component(namespace, "namespace")?))
    }

    fn table_dir(
        &self,
        namespace: &str,
        table: &str,
        kind: EntryKind,
    ) -> Result<PathBuf, DomainError> {
        let directory = match kind {
            EntryKind::Json => "kv",
            EntryKind::Blob => "blobs",
        };
        Ok(self
            .namespace_root(namespace)?
            .join(directory)
            .join(validate_component(table, "table")?))
    }

    fn entry_path(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        kind: EntryKind,
    ) -> Result<PathBuf, DomainError> {
        let key = validate_component(key, "key")?;
        let directory = self.table_dir(namespace, table, kind)?;
        Ok(match kind {
            EntryKind::Json => directory.join(format!("{key}.json")),
            EntryKind::Blob => directory.join(key),
        })
    }
}

fn validate_component<'a>(raw: &'a str, label: &str) -> Result<&'a str, DomainError> {
    let value = raw.trim();
    if value.is_empty()
        || value.starts_with('.')
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(DomainError::InvalidData(format!(
            "Invalid extension store {label}: use [A-Za-z0-9_.-], without a leading dot"
        )));
    }
    Ok(value)
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> DomainError {
    if source.kind() == io::ErrorKind::NotFound {
        DomainError::NotFound(format!(
            "Extension store entry {}: {source}",
            path.display()
        ))
    } else {
        DomainError::FileIo {
            operation,
            path: path.display().to_string(),
            source,
        }
    }
}

/// The receiving file is private until validation and atomic replacement succeed.
fn publish(session: CommitSession<WriteTarget>) -> Result<(), DomainError> {
    let received = session.stage.path().to_path_buf();
    let merged = session.stage.publish_path();
    drop(session.stage);
    let WriteTarget { path, operation } = session.metadata;

    let source = match operation {
        WriteOperation::SetJson => {
            json::validate(&received, &path)?;
            &received
        }
        WriteOperation::UpdateJson => {
            json::merge(&path, &received, &merged)?;
            &merged
        }
        WriteOperation::SetBlob => &received,
    };
    let parent = path.parent().expect("entry has a table directory");
    std::fs::create_dir_all(parent).map_err(|error| io_error("create directory", parent, error))?;
    replace_file_blocking(source, &path)
}

#[async_trait]
impl ExtensionStoreRepository for FileExtensionStoreRepository {
    async fn open_entry(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        kind: EntryKind,
    ) -> Result<Option<Box<dyn ByteReader>>, DomainError> {
        let path = self.entry_path(namespace, table, key, kind)?;
        let file = match fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error("open", &path, error)),
        };
        Ok(Some(Box::new(EntryReader { file, path })))
    }

    async fn begin_commit(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        operation: WriteOperation,
    ) -> Result<CommitBegin, DomainError> {
        let path = self.entry_path(namespace, table, key, operation.kind())?;
        self.commits.begin(WriteTarget { path, operation }).await
    }

    async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, DomainError> {
        self.commits.append(session_id, offset, bytes, |_| {}).await
    }

    async fn finish_commit(&self, session_id: &str, expected_size: u64) -> Result<(), DomainError> {
        self.commits
            .finish(session_id, expected_size, |session| async move {
                let guard = self.mutation.clone().lock_owned().await;
                tokio::task::spawn_blocking(move || {
                    // Keep the lock with the IO task even if its awaiting caller disappears.
                    let _guard = guard;
                    publish(session)
                })
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Extension store commit task failed: {error}"
                    ))
                })?
            })
            .await
    }

    async fn abort_commit(&self, session_id: &str) -> Result<(), DomainError> {
        self.commits.abort(session_id).await
    }

    async fn rename_json_key(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError> {
        let from = self.entry_path(namespace, table, key, EntryKind::Json)?;
        let to = self.entry_path(namespace, table, new_key, EntryKind::Json)?;
        if from == to {
            return Ok(());
        }
        let _guard = self.mutation.lock().await;
        if fs::try_exists(&to)
            .await
            .map_err(|error| io_error("stat", &to, error))?
        {
            return Err(DomainError::InvalidData(format!(
                "Extension store JSON entry already exists: {}",
                to.display()
            )));
        }
        fs::rename(&from, &to)
            .await
            .map_err(|error| io_error("rename", &from, error))
    }

    async fn delete_entry(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        kind: EntryKind,
    ) -> Result<(), DomainError> {
        let path = self.entry_path(namespace, table, key, kind)?;
        let _guard = self.mutation.lock().await;
        fs::remove_file(&path)
            .await
            .map_err(|error| io_error("delete", &path, error))
    }

    async fn list_keys(
        &self,
        namespace: &str,
        table: &str,
        kind: EntryKind,
    ) -> Result<Vec<String>, DomainError> {
        let directory = self.table_dir(namespace, table, kind)?;
        let names = list_names(&directory, std::fs::FileType::is_file).await?;
        Ok(match kind {
            EntryKind::Json => names
                .into_iter()
                .filter_map(|name| name.strip_suffix(".json").map(str::to_owned))
                .collect(),
            EntryKind::Blob => names,
        })
    }

    async fn list_tables(&self, namespace: &str) -> Result<Vec<String>, DomainError> {
        let root = self.namespace_root(namespace)?;
        let mut tables = list_names(&root.join("kv"), std::fs::FileType::is_dir).await?;
        tables.extend(list_names(&root.join("blobs"), std::fs::FileType::is_dir).await?);
        tables.sort();
        tables.dedup();
        Ok(tables)
    }

    async fn delete_table(&self, namespace: &str, table: &str) -> Result<(), DomainError> {
        let json = self.table_dir(namespace, table, EntryKind::Json)?;
        let blobs = self.table_dir(namespace, table, EntryKind::Blob)?;
        let _guard = self.mutation.lock().await;
        let mut removed = false;
        for directory in [json, blobs] {
            match fs::remove_dir_all(&directory).await {
                Ok(()) => removed = true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error("delete table", &directory, error)),
            }
        }
        if !removed {
            return Err(DomainError::NotFound(format!(
                "Extension store table not found: {namespace}:{table}"
            )));
        }
        Ok(())
    }
}

async fn list_names(
    directory: &Path,
    accept_type: fn(&std::fs::FileType) -> bool,
) -> Result<Vec<String>, DomainError> {
    let mut entries = match fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error("list directory", directory, error)),
    };
    let mut names = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| io_error("list directory", directory, error))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|error| io_error("inspect entry", &entry.path(), error))?;
        if !accept_type(&file_type) {
            continue;
        }
        let name = entry.file_name();
        if let Some(name) = name.to_str()
            && validate_component(name, "entry").is_ok_and(|normalized| normalized == name)
        {
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}
