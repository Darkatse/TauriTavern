use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OwnedRwLockWriteGuard;
use tt_ports::database::DatabaseFileAccess;
use ttsync_core::database::namespace_directory;
use ttsync_fs::databases::DatabaseTransfer;

use tokio::io::AsyncRead;
use ttsync_client::{ClientWorkspace, WorkspaceWriteError};
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;

use crate::sync::http_client::domain_error_to_sync;
use crate::tt_sync::fs::scan_manifest_with_policy;
use crate::{sync_fs, sync_transfer};

pub struct TauriTavernSyncWorkspace {
    sync_root: PathBuf,
    database: Arc<dyn DatabaseFileAccess>,
    transfer: Option<DatabaseTransfer>,
    _guard: Option<OwnedRwLockWriteGuard<()>>,
}

impl TauriTavernSyncWorkspace {
    pub fn new(sync_root: PathBuf, database: Arc<dyn DatabaseFileAccess>) -> Self {
        Self {
            sync_root,
            database,
            transfer: None,
            _guard: None,
        }
    }

    fn resolve(&self, path: &SyncPath) -> PathBuf {
        sync_transfer::resolve_to_local(&self.sync_root, path)
    }
}

impl ClientWorkspace for TauriTavernSyncWorkspace {
    async fn prepare(
        self: Arc<Self>,
        policy: ResolvedDatasetPolicy,
        receiving: bool,
    ) -> Result<Arc<Self>, SyncError> {
        if !policy
            .selection()
            .dataset_ids
            .iter()
            .any(|id| id == ttsync_core::database::DATASET_ID)
        {
            return Ok(self);
        }
        let guard = self
            .database
            .prepare_sync(receiving)
            .await
            .map_err(domain_error_to_sync)?;
        Ok(Arc::new(Self {
            sync_root: self.sync_root.clone(),
            database: self.database.clone(),
            transfer: receiving.then(|| DatabaseTransfer::new(&self.sync_root)),
            _guard: Some(guard),
        }))
    }

    fn set_plan(&self, plan: &ttsync_contract::plan::SyncPlan) {
        if let Some(transfer) = &self.transfer {
            transfer.set_plan(plan);
        }
    }

    fn is_applied(&self, path: &str) -> bool {
        self.transfer
            .as_ref()
            .is_none_or(|transfer| transfer.is_applied(path))
    }

    async fn commit(self: Arc<Self>) -> Result<(), WorkspaceWriteError> {
        if self.transfer.is_none() {
            return Ok(());
        }
        // The whole prepared workspace owns the maintenance lock through the blocking swap.
        tokio::task::spawn_blocking(move || {
            let transfer = self.transfer.as_ref().unwrap();
            transfer.commit().map_err(|error| {
                if transfer.changed() {
                    WorkspaceWriteError::changed(error)
                } else {
                    WorkspaceWriteError::unchanged(error)
                }
            })
        })
        .await
        .map_err(|error| WorkspaceWriteError::changed(SyncError::Internal(error.to_string())))?
    }

    async fn scan(&self, policy: ResolvedDatasetPolicy) -> Result<ManifestV2, SyncError> {
        scan_manifest_with_policy(self.sync_root.clone(), policy)
            .await
            .map_err(domain_error_to_sync)
    }

    async fn read_file(
        &self,
        path: &SyncPath,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>, SyncError> {
        let file = tokio::fs::File::open(self.resolve(path))
            .await
            .map_err(|error| SyncError::Io(error.to_string()))?;
        Ok(Box::new(file))
    }

    async fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> Result<(), WorkspaceWriteError> {
        if let Some(transfer) = &self.transfer
            && namespace_directory(path.as_str()).is_some()
        {
            return transfer
                .write_file(path, data, modified_ms)
                .await
                .map_err(WorkspaceWriteError::unchanged);
        }
        sync_fs::write_file_atomic(&self.resolve(path), data, modified_ms)
            .await
            .map_err(workspace_write_error)
    }

    async fn delete_file(&self, path: &SyncPath) -> Result<(), WorkspaceWriteError> {
        if self.transfer.is_some() && namespace_directory(path.as_str()).is_some() {
            return Ok(());
        }
        sync_fs::delete_sync_file(&self.sync_root, path)
            .await
            .map_err(workspace_write_error)
    }
}

// The LAN server uses the same storage lifetime and file rules as active Sync jobs.
impl ttsync_core::ports::ManifestStore for TauriTavernSyncWorkspace {
    async fn prepare(
        self: Arc<Self>,
        policy: ResolvedDatasetPolicy,
        receiving: bool,
    ) -> Result<Arc<Self>, SyncError> {
        ClientWorkspace::prepare(self, policy, receiving).await
    }
    fn set_plan(&self, plan: &ttsync_contract::plan::SyncPlan) {
        ClientWorkspace::set_plan(self, plan)
    }
    async fn commit(self: Arc<Self>) -> Result<(), SyncError> {
        ClientWorkspace::commit(self)
            .await
            .map_err(WorkspaceWriteError::into_error)
    }
    async fn scan(&self, policy: ResolvedDatasetPolicy) -> Result<ManifestV2, SyncError> {
        ClientWorkspace::scan(self, policy).await
    }
    async fn read_file(
        &self,
        path: &SyncPath,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>, SyncError> {
        ClientWorkspace::read_file(self, path).await
    }
    async fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> Result<(), SyncError> {
        ClientWorkspace::write_file(self, path, data, modified_ms)
            .await
            .map_err(WorkspaceWriteError::into_error)
    }
    async fn delete_file(&self, path: &SyncPath) -> Result<(), SyncError> {
        ClientWorkspace::delete_file(self, path)
            .await
            .map_err(WorkspaceWriteError::into_error)
    }
}

#[cfg(test)]
pub(crate) fn empty_database_access() -> Arc<dyn DatabaseFileAccess> {
    struct NoOpenDatabases;
    #[async_trait::async_trait]
    impl DatabaseFileAccess for NoOpenDatabases {
        async fn prepare_sync(
            &self,
            _: bool,
        ) -> Result<OwnedRwLockWriteGuard<()>, tt_domain::errors::DomainError> {
            Ok(Arc::new(tokio::sync::RwLock::new(())).write_owned().await)
        }
    }
    Arc::new(NoOpenDatabases)
}

fn workspace_write_error(error: sync_fs::FileMutationError) -> WorkspaceWriteError {
    let target_changed = error.target_changed();
    let error = error.into_error();
    if target_changed {
        WorkspaceWriteError::changed(error)
    } else {
        WorkspaceWriteError::unchanged(error)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;

    use tokio::io::AsyncReadExt;
    use ttsync_client::ClientWorkspace;
    use ttsync_contract::path::SyncPath;
    use uuid::Uuid;

    use super::TauriTavernSyncWorkspace;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-sync-workspace-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn workspace_round_trips_file_operations_and_prunes_empty_parents() {
        let root = temp_root();
        let workspace = TauriTavernSyncWorkspace::new(root.clone(), super::empty_database_access());
        let path = SyncPath::new("default-user/chats/thread/hello.json".to_string()).unwrap();
        let mut source = Cursor::new(br#"{"hello":true}"#.to_vec());

        workspace
            .write_file(&path, &mut source, 1_000)
            .await
            .unwrap();

        let mut reader = workspace.read_file(&path).await.unwrap();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(&bytes, br#"{"hello":true}"#);

        workspace.delete_file(&path).await.unwrap();
        assert!(!root.join("default-user/chats/thread").exists());
        assert!(root.join("default-user/chats").exists());

        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
