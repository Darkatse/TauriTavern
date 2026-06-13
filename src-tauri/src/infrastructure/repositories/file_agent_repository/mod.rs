mod checkpoint_store;
mod event_journal;
mod fs_tree;
mod invocation_store;
mod lifecycle_store;
mod paths;
mod persistent_store;
mod run_prune_store;
mod run_store;
mod workspace_store;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::fs::read_to_string;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file};

pub struct FileAgentRepository {
    pub(super) root: PathBuf,
    pub(super) event_lock: Arc<Mutex<()>>,
    pub(super) checkpoint_lock: Arc<Mutex<()>>,
    pub(super) persist_lock: Arc<Mutex<()>>,
    pub(super) workspace_write_locks: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>>,
}

impl FileAgentRepository {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            event_lock: Arc::new(Mutex::new(())),
            checkpoint_lock: Arc::new(Mutex::new(())),
            persist_lock: Arc::new(Mutex::new(())),
            workspace_write_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn write_json_atomic<T: Serialize + ?Sized>(
        path: &Path,
        value: &T,
    ) -> Result<(), DomainError> {
        write_json_file(path, value).await
    }

    async fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, DomainError> {
        read_json_file(path).await
    }

    async fn try_read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, DomainError> {
        let contents = match read_to_string(path).await {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read file {}: {}",
                    path.display(),
                    error
                )));
            }
        };

        serde_json::from_str(&contents).map(Some).map_err(|error| {
            DomainError::InvalidData(format!("Invalid JSON in {}: {}", path.display(), error))
        })
    }
}
