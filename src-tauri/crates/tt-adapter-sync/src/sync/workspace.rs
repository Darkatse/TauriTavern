use std::path::PathBuf;

use tokio::io::AsyncRead;
use ttsync_client::{ClientWorkspace, WorkspaceWriteError};
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;

use crate::sync::http_client::domain_error_to_sync;
use crate::tt_sync::fs::scan_manifest_with_policy;
use crate::{sync_fs, sync_transfer};

#[derive(Debug)]
pub struct TauriTavernSyncWorkspace {
    sync_root: PathBuf,
}

impl TauriTavernSyncWorkspace {
    pub fn new(sync_root: PathBuf) -> Self {
        Self { sync_root }
    }

    fn resolve(&self, path: &SyncPath) -> PathBuf {
        sync_transfer::resolve_to_local(&self.sync_root, path)
    }
}

impl ClientWorkspace for TauriTavernSyncWorkspace {
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
        sync_fs::write_file_atomic(&self.resolve(path), data, modified_ms)
            .await
            .map_err(|error| {
                let target_changed = error.target_changed();
                let error = domain_error_to_sync(error.into_error());
                if target_changed {
                    WorkspaceWriteError::changed(error)
                } else {
                    WorkspaceWriteError::unchanged(error)
                }
            })
    }

    async fn delete_file(&self, path: &SyncPath) -> Result<(), WorkspaceWriteError> {
        tokio::fs::remove_file(self.resolve(path))
            .await
            .map_err(|error| WorkspaceWriteError::unchanged(SyncError::Io(error.to_string())))
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
    async fn workspace_round_trips_file_operations() {
        let root = temp_root();
        let workspace = TauriTavernSyncWorkspace::new(root.clone());
        let path = SyncPath::new("default-user/chats/hello.json".to_string()).unwrap();
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
        assert!(!root.join("default-user/chats/hello.json").exists());

        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
