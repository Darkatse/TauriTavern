use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, Notify, watch};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;
use tt_ports::workspace_fs::{
    WorkspaceAppendResult, WorkspaceDirectoryEntry, WorkspaceEntryKind, WorkspaceFs,
    WorkspaceMetadata, WorkspaceWriteGuard,
};
use tt_ports::workspace_shell::{WorkspaceShell, WorkspaceShellExit, WorkspaceShellRequest};

use crate::BashkitWorkspaceShell;

/// A controlled write future: the test decides when the pending side effect completes.
#[derive(Default)]
struct BlockedWorkspace {
    started: Notify,
    release: Notify,
    files: Mutex<HashMap<String, Vec<u8>>>,
}

#[async_trait]
impl WorkspaceFs for BlockedWorkspace {
    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError> {
        let (kind, bytes) = match path {
            None => (WorkspaceEntryKind::Directory, 0),
            Some(path) => {
                let files = self.files.lock().await;
                let bytes = files
                    .get(path.as_str())
                    .ok_or_else(|| DomainError::NotFound(path.as_str().to_string()))?;
                (WorkspaceEntryKind::File, bytes.len() as u64)
            }
        };
        Ok(WorkspaceMetadata {
            kind,
            bytes,
            modified: None,
            created: None,
        })
    }

    async fn write_file(
        &self,
        path: &WorkspacePath,
        bytes: &[u8],
        _: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        if path.as_str() == "draft.md" && !bytes.is_empty() {
            self.started.notify_one();
            self.release.notified().await;
        }
        self.files
            .lock()
            .await
            .insert(path.as_str().to_string(), bytes.to_vec());
        Ok(())
    }

    async fn read_file(&self, _: &WorkspacePath, _: usize) -> Result<Vec<u8>, DomainError> {
        unreachable!("the cancellation script only writes files")
    }
    async fn append_file(&self, _: &WorkspacePath, _: &[u8]) -> Result<(), DomainError> {
        unreachable!("the cancellation script only replaces files")
    }
    async fn append_text(
        &self,
        _: &WorkspacePath,
        _: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        unreachable!("the shell uses byte writes")
    }
    async fn read_dir(
        &self,
        _: Option<&WorkspacePath>,
        _: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        unreachable!("the cancellation script does not list directories")
    }
    async fn create_dir(&self, _: &WorkspacePath, _: bool) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not create directories")
    }
    async fn remove(&self, _: &WorkspacePath, _: bool) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not remove files")
    }
    async fn rename(&self, _: &WorkspacePath, _: &WorkspacePath) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not move files")
    }
    async fn copy_file(&self, _: &WorkspacePath, _: &WorkspacePath) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not copy files")
    }
}

#[tokio::test]
async fn cancellation_finishes_current_write_and_stops_following_commands() {
    let files = Arc::new(BlockedWorkspace::default());
    let (cancel, receiver) = watch::channel(false);
    let request = WorkspaceShellRequest {
        command: "printf draft > /draft.md; printf later > /later.md".to_string(),
        workdir: "/".to_string(),
        files: files.clone(),
        cancel: receiver,
    };
    let task = tokio::spawn(async move { BashkitWorkspaceShell.execute(request).await });
    files.started.notified().await;
    cancel.send(true).unwrap();
    files.release.notify_one();

    let result = task.await.unwrap().unwrap();
    assert_eq!(result.exit, WorkspaceShellExit::Cancelled, "{result:?}");
    let written = files.files.lock().await;
    assert_eq!(written.get("draft.md").unwrap(), b"draft");
    assert!(!written.contains_key("later.md"));
}
