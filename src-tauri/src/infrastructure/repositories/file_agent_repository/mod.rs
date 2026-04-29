mod checkpoint_store;
mod event_journal;
mod fs_tree;
mod paths;
mod persistent_store;
mod run_store;
mod workspace_store;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file};

pub struct FileAgentRepository {
    pub(super) root: PathBuf,
    pub(super) event_lock: Arc<Mutex<()>>,
    pub(super) checkpoint_lock: Arc<Mutex<()>>,
    pub(super) persist_lock: Arc<Mutex<()>>,
}

impl FileAgentRepository {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            event_lock: Arc::new(Mutex::new(())),
            checkpoint_lock: Arc::new(Mutex::new(())),
            persist_lock: Arc::new(Mutex::new(())),
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
}
