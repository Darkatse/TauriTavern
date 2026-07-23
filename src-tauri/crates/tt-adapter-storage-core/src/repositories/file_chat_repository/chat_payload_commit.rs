use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_payload_commit_repository::{
    ChatPayloadCommitBegin, ChatPayloadCommitRepository, ChatPayloadTarget, CommittedChatPayload,
};
use uuid::Uuid;

use super::integrity::verify_integrity_match;
use super::{ContentSignature, FileChatRepository};

const MOBILE_MAX_FRAME_BYTES: u64 = 1024 * 1024;
const DESKTOP_MAX_FRAME_BYTES: u64 = 4 * 1024 * 1024;
pub(super) const MAX_ACTIVE_CHAT_COMMIT_SESSIONS: usize = 8;

// Keep digest semantics deterministic in unit tests; production only hashes on hardware backends.
#[cfg(test)]
fn new_content_hasher() -> Option<Sha256> {
    Some(Sha256::new())
}

#[cfg(all(not(test), target_arch = "aarch64", not(target_os = "windows")))]
fn new_content_hasher() -> Option<Sha256> {
    std::arch::is_aarch64_feature_detected!("sha2").then(Sha256::new)
}

#[cfg(all(not(test), any(target_arch = "x86", target_arch = "x86_64")))]
fn new_content_hasher() -> Option<Sha256> {
    (std::arch::is_x86_feature_detected!("sha")
        && std::arch::is_x86_feature_detected!("sse2")
        && std::arch::is_x86_feature_detected!("ssse3")
        && std::arch::is_x86_feature_detected!("sse4.1"))
    .then(Sha256::new)
}

#[cfg(all(
    not(test),
    not(any(
        all(target_arch = "aarch64", not(target_os = "windows")),
        target_arch = "x86",
        target_arch = "x86_64"
    ))
))]
fn new_content_hasher() -> Option<Sha256> {
    None
}

pub(super) struct CommitSession {
    target: ChatPayloadTarget,
    target_path: PathBuf,
    stage_path: PathBuf,
    file: Option<fs::File>,
    content_hasher: Option<(u64, Sha256)>,
    accepted_offset: u64,
    force: bool,
}

impl FileChatRepository {
    pub async fn cleanup_orphaned_chat_commit_staging(&self) -> Result<(), DomainError> {
        match fs::remove_dir_all(&self.chat_commit_staging_dir).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DomainError::InternalError(format!(
                "Failed to clean chat commit staging directory {}: {}",
                self.chat_commit_staging_dir.display(),
                error
            ))),
        }
    }

    fn chat_commit_max_frame_bytes() -> u64 {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            MOBILE_MAX_FRAME_BYTES
        } else {
            DESKTOP_MAX_FRAME_BYTES
        }
    }

    fn parse_chat_commit_session_id(session_id: &str) -> Result<Uuid, DomainError> {
        Uuid::parse_str(session_id)
            .map_err(|_| DomainError::InvalidData("Invalid chat commit session id".to_string()))
    }

    async fn resolve_chat_commit_target(
        &self,
        target: &ChatPayloadTarget,
    ) -> Result<PathBuf, DomainError> {
        match target {
            ChatPayloadTarget::Character {
                character_id,
                file_name,
            } => {
                self.resolve_character_chat_path(character_id, file_name)
                    .await
            }
            ChatPayloadTarget::Group { chat_id } => self.get_group_chat_path(chat_id),
        }
    }

    async fn remove_chat_commit_stage(&self, stage_path: &Path) {
        match fs::remove_file(stage_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => tracing::error!(
                path = %stage_path.display(),
                error = %error,
                "Failed to remove rejected chat commit stage",
            ),
        }
    }

    async fn strict_publish_chat_payload(
        stage_path: &Path,
        target_path: &Path,
    ) -> Result<(), DomainError> {
        fs::rename(stage_path, target_path).await.map_err(|error| {
            tracing::error!(
                stage = %stage_path.display(),
                target = %target_path.display(),
                error = %error,
                "Failed to publish chat payload",
            );
            DomainError::InternalError(format!("Failed to publish chat payload: {error}"))
        })
    }
}

#[async_trait]
impl ChatPayloadCommitRepository for FileChatRepository {
    async fn begin(
        &self,
        target: ChatPayloadTarget,
        force: bool,
    ) -> Result<ChatPayloadCommitBegin, DomainError> {
        let target_path = self.resolve_chat_commit_target(&target).await?;
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat payload directory {}: {}",
                    parent.display(),
                    error
                ))
            })?;
        }
        fs::create_dir_all(&self.chat_commit_staging_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat commit staging directory {}: {}",
                    self.chat_commit_staging_dir.display(),
                    error
                ))
            })?;

        let session_id = Uuid::new_v4();
        let stage_path = self
            .chat_commit_staging_dir
            .join(format!("{session_id}.partial"));
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&stage_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat commit stage {}: {}",
                    stage_path.display(),
                    error
                ))
            })?;
        let max_frame_bytes = Self::chat_commit_max_frame_bytes();
        let should_hash = self
            .backup_policy
            .try_read()
            .is_ok_and(|policy| policy.automatic_enabled && !policy.history_disabled());
        let content_hasher = if should_hash {
            match new_content_hasher() {
                Some(hasher) => Some((self.current_content_signature_epoch().await, hasher)),
                None => None,
            }
        } else {
            None
        };
        let session = Arc::new(Mutex::new(CommitSession {
            target,
            target_path,
            stage_path: stage_path.clone(),
            file: Some(file),
            content_hasher,
            accepted_offset: 0,
            force,
        }));

        let mut sessions = self.chat_commit_sessions.lock().await;
        if sessions.len() >= MAX_ACTIVE_CHAT_COMMIT_SESSIONS {
            drop(sessions);
            drop(session);
            self.remove_chat_commit_stage(&stage_path).await;
            return Err(DomainError::Conflict(
                "Too many active chat commit sessions".to_string(),
            ));
        }
        sessions.insert(session_id, session);
        drop(sessions);

        Ok(ChatPayloadCommitBegin {
            session_id: session_id.to_string(),
            max_frame_bytes,
        })
    }

    async fn append(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, DomainError> {
        let parsed_session_id = Self::parse_chat_commit_session_id(session_id)?;
        let session = self
            .chat_commit_sessions
            .lock()
            .await
            .get(&parsed_session_id)
            .cloned()
            .ok_or_else(|| {
                DomainError::NotFound(format!("Chat commit session not found: {session_id}"))
            })?;
        let mut session = session.lock().await;

        if bytes.is_empty() {
            return Err(DomainError::InvalidData(
                "Chat commit frame cannot be empty".to_string(),
            ));
        }
        let max_frame_bytes = Self::chat_commit_max_frame_bytes();
        if bytes.len() as u64 > max_frame_bytes {
            return Err(DomainError::InvalidData(format!(
                "Chat commit frame exceeds {} bytes",
                max_frame_bytes
            )));
        }
        if offset != session.accepted_offset {
            return Err(DomainError::InvalidData(format!(
                "Chat commit offset mismatch: expected {}, got {}",
                session.accepted_offset, offset
            )));
        }

        let file = session.file.as_mut().ok_or_else(|| {
            DomainError::Conflict(format!(
                "Chat commit session no longer accepts chunks: {session_id}"
            ))
        })?;
        file.write_all(bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to append chat commit session {session_id}: {error}"
            ))
        })?;
        if let Some((_, content_hasher)) = session.content_hasher.as_mut() {
            content_hasher.update(bytes);
        }
        session.accepted_offset += bytes.len() as u64;
        Ok(session.accepted_offset)
    }

    async fn finish(
        &self,
        session_id: &str,
        expected_size: u64,
    ) -> Result<CommittedChatPayload, DomainError> {
        let parsed_session_id = Self::parse_chat_commit_session_id(session_id)?;
        let session = self
            .chat_commit_sessions
            .lock()
            .await
            .remove(&parsed_session_id)
            .ok_or_else(|| {
                DomainError::NotFound(format!("Chat commit session not found: {session_id}"))
            })?;
        let mut session = session.lock().await;
        let target = session.target.clone();
        let target_path = session.target_path.clone();
        let stage_path = session.stage_path.clone();
        let accepted_offset = session.accepted_offset;
        let content_hasher = session.content_hasher.take();
        let force = session.force;
        let mut file = session
            .file
            .take()
            .expect("claimed chat commit session must own an open stage");
        drop(session);

        let result = async {
            file.flush().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to flush chat commit session {session_id}: {error}"
                ))
            })?;
            drop(file);

            let actual_size = fs::metadata(&stage_path)
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to stat chat commit stage {}: {}",
                        stage_path.display(),
                        error
                    ))
                })?
                .len();
            if expected_size != accepted_offset || actual_size != accepted_offset {
                return Err(DomainError::InvalidData(format!(
                    "Chat commit size mismatch: expected {expected_size}, accepted {accepted_offset}, staged {actual_size}"
                )));
            }
            let content_signature = content_hasher.map(|(epoch, content_hasher)| {
                (
                    epoch,
                    ContentSignature {
                        byte_len: accepted_offset,
                        sha256: content_hasher.finalize().into(),
                    },
                )
            });

            let incoming_integrity =
                Self::read_incoming_integrity_from_file(&stage_path).await?;
            let character_cache_key = match &target {
                ChatPayloadTarget::Character {
                    character_id,
                    file_name,
                } => Some(self.get_cache_key(character_id, file_name)?),
                ChatPayloadTarget::Group { .. } => None,
            };

            let _write_guard = self.acquire_payload_mutation_lock(&target_path).await;
            if !force {
                let existing_integrity = self
                    .read_integrity_slug_from_existing_file(&target_path)
                    .await?;
                verify_integrity_match(
                    existing_integrity.as_deref(),
                    incoming_integrity.as_deref(),
                )?;
            }

            Self::strict_publish_chat_payload(&stage_path, &target_path).await?;
            if let Some((epoch, content_signature)) = content_signature {
                self.record_current_content_signature(&target_path, epoch, content_signature)
                    .await;
            }
            drop(_write_guard);

            if let Some(cache_key) = character_cache_key {
                self.memory_cache.lock().await.remove(&cache_key);
            }
            self.remove_summary_cache_for_path(&target_path).await;

            Ok(CommittedChatPayload {
                target,
                size: expected_size,
            })
        }
        .await;

        if result.is_err() {
            self.remove_chat_commit_stage(&stage_path).await;
        }
        result
    }

    async fn abort(&self, session_id: &str) -> Result<(), DomainError> {
        let parsed_session_id = Self::parse_chat_commit_session_id(session_id)?;
        let Some(session) = self
            .chat_commit_sessions
            .lock()
            .await
            .remove(&parsed_session_id)
        else {
            return Ok(());
        };
        let mut session = session.lock().await;
        let stage_path = session.stage_path.clone();
        let file = session.file.take();
        drop(session);
        drop(file);

        match fs::remove_file(&stage_path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DomainError::InternalError(format!(
                "Failed to abort chat commit session {session_id}: {error}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chat_commit_missing_stage_never_guesses_success_from_target_size() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-chat-strict-publish-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).await.expect("create temp root");
        let stage = root.join("missing-stage");
        let target = root.join("target");
        fs::write(&target, b"old").await.expect("write target");

        FileChatRepository::strict_publish_chat_payload(&stage, &target)
            .await
            .expect_err("a missing stage must remain a failed publish");
        assert_eq!(fs::read(&target).await.expect("read target"), b"old");

        fs::remove_dir_all(root).await.expect("remove temp root");
    }
}
