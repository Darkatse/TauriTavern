//! Bounded byte commits: session ownership, received files and cleanup.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tt_contracts::byte_commit::CommitBegin;
use tt_domain::errors::DomainError;
use uuid::Uuid;

const MAX_ACTIVE_SESSIONS: usize = 8;

pub struct CommitSession<M> {
    pub stage: CommitStage,
    pub metadata: M,
}

type ActiveSession<M> = Arc<Mutex<Option<CommitSession<M>>>>;

pub struct CommitSessions<M> {
    directory: PathBuf,
    active: Mutex<HashMap<Uuid, ActiveSession<M>>>,
}

impl<M> CommitSessions<M> {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            active: Mutex::new(HashMap::new()),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Called at startup, before accepting commits. Orphans do not affect published data.
    pub async fn cleanup_orphans(&self) {
        if let Err(error) = fs::remove_dir_all(&self.directory).await
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.directory.display(), %error, "Failed to clean commit staging");
        }
    }

    pub async fn begin(&self, metadata: M) -> Result<CommitBegin, DomainError> {
        fs::create_dir_all(&self.directory)
            .await
            .map_err(|error| stage_error(&self.directory, "create directory for", error))?;
        let id = Uuid::new_v4();
        let path = self.directory.join(format!("{id}.partial"));
        let stage = CommitStage::create(path.clone()).await?;
        let mut active = self.active.lock().await;
        if active.len() >= MAX_ACTIVE_SESSIONS {
            drop(active);
            drop(stage);
            cleanup_stage(&path).await;
            return Err(DomainError::Conflict(
                "Too many active commit sessions".into(),
            ));
        }
        active.insert(
            id,
            Arc::new(Mutex::new(Some(CommitSession { stage, metadata }))),
        );
        Ok(CommitBegin {
            session_id: id.to_string(),
            max_frame_bytes: max_frame_bytes(),
        })
    }

    pub async fn append(
        &self,
        id: &str,
        offset: u64,
        bytes: &[u8],
        on_accepted: impl FnOnce(&mut M),
    ) -> Result<u64, DomainError> {
        let session = self
            .active
            .lock()
            .await
            .get(&session_id(id)?)
            .cloned()
            .ok_or_else(|| missing_session(id))?;
        let mut active = session.lock().await;
        let session = active.as_mut().ok_or_else(|| missing_session(id))?;
        let accepted = session.stage.append(offset, bytes).await?;
        on_accepted(&mut session.metadata);
        Ok(accepted)
    }

    pub async fn finish<T, F: Future<Output = Result<T, DomainError>>>(
        &self,
        id: &str,
        expected_size: u64,
        publish: impl FnOnce(CommitSession<M>) -> F,
    ) -> Result<T, DomainError> {
        let mut session = self.take(id).await?.ok_or_else(|| missing_session(id))?;
        let path = session.stage.path.clone();
        let publish_path = session.stage.publish_path();
        let result = async {
            session.stage.validate_size(expected_size).await?;
            publish(session).await
        }
        .await;
        // Cleanup cannot change a confirmed publication or replace its original error.
        cleanup_stage(&path).await;
        cleanup_stage(&publish_path).await;
        result
    }

    pub async fn abort(&self, id: &str) -> Result<(), DomainError> {
        if let Some(session) = self.take(id).await? {
            let path = session.stage.path.clone();
            drop(session);
            remove_stage(&path).await?;
        }
        Ok(())
    }

    async fn take(&self, id: &str) -> Result<Option<CommitSession<M>>, DomainError> {
        let Some(session) = self.active.lock().await.remove(&session_id(id)?) else {
            return Ok(None);
        };
        Ok(Some(
            session
                .lock()
                .await
                .take()
                .expect("claimed commit session must be active"),
        ))
    }
}

async fn remove_stage(path: &Path) -> Result<(), DomainError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(stage_error(path, "remove", error)),
    }
}

async fn cleanup_stage(path: &Path) {
    if let Err(error) = remove_stage(path).await {
        tracing::warn!(%error, "Failed to clean commit stage");
    }
}

fn session_id(id: &str) -> Result<Uuid, DomainError> {
    Uuid::parse_str(id).map_err(|_| DomainError::InvalidData("Invalid commit session id".into()))
}

fn missing_session(id: &str) -> DomainError {
    DomainError::NotFound(format!("Commit session is no longer active: {id}"))
}

fn max_frame_bytes() -> u64 {
    if cfg!(target_os = "android") {
        256 * 1024
    } else if cfg!(target_os = "ios") {
        1024 * 1024
    } else {
        4 * 1024 * 1024
    }
}

pub struct CommitStage {
    pub(crate) path: PathBuf,
    pub(crate) file: File,
    pub(crate) accepted_offset: u64,
}

impl CommitStage {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn publish_path(&self) -> PathBuf {
        self.path.with_extension("publish")
    }

    async fn create(path: PathBuf) -> Result<Self, DomainError> {
        let file = File::options()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
            .map_err(|error| stage_error(&path, "create", error))?;
        Ok(Self {
            path,
            file,
            accepted_offset: 0,
        })
    }

    async fn append(&mut self, offset: u64, bytes: &[u8]) -> Result<u64, DomainError> {
        if bytes.is_empty() || bytes.len() as u64 > max_frame_bytes() {
            return Err(DomainError::InvalidData(format!(
                "Commit frame must contain 1..={} bytes",
                max_frame_bytes()
            )));
        }
        if offset != self.accepted_offset {
            return Err(DomainError::InvalidData(format!(
                "Commit offset mismatch: expected {}, got {offset}",
                self.accepted_offset
            )));
        }
        self.file
            .write_all(bytes)
            .await
            .map_err(|error| stage_error(&self.path, "append", error))?;
        self.accepted_offset += bytes.len() as u64;
        Ok(self.accepted_offset)
    }

    async fn validate_size(&mut self, expected_size: u64) -> Result<(), DomainError> {
        self.file
            .flush()
            .await
            .map_err(|error| stage_error(&self.path, "flush", error))?;
        let actual_size = self
            .file
            .metadata()
            .await
            .map_err(|error| stage_error(&self.path, "stat", error))?
            .len();
        if expected_size != self.accepted_offset || actual_size != self.accepted_offset {
            return Err(DomainError::InvalidData(format!(
                "Commit size mismatch: expected {expected_size}, accepted {}, staged {actual_size}",
                self.accepted_offset
            )));
        }
        Ok(())
    }
}

fn stage_error(path: &std::path::Path, operation: &str, error: std::io::Error) -> DomainError {
    DomainError::InternalError(format!(
        "Failed to {operation} commit stage {}: {error}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn commit_cleanup_failure_keeps_published_result() {
        let root = std::env::temp_dir().join(format!("commit-cleanup-{}", Uuid::new_v4()));
        let sessions = CommitSessions::new(root.join("staging"));
        let target = root.join("published");
        let id = sessions.begin(()).await.unwrap().session_id;
        sessions.append(&id, 0, b"saved", |()| {}).await.unwrap();
        sessions
            .finish(&id, 5, |session| async {
                let path = session.stage.path.clone();
                drop(session);
                fs::rename(&path, &target).await.unwrap();
                // A directory cannot be removed as a file: force cleanup to fail after publication.
                fs::create_dir(&path).await.unwrap();
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(fs::read(&target).await.unwrap(), b"saved");
        fs::remove_dir_all(root).await.unwrap();
    }
}
