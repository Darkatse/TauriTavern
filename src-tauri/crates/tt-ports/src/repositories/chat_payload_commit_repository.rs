use super::chat_repository::ChatByteReader;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tt_domain::errors::DomainError;

/// An opened chat file retained by the current page, independent of later path replacements.
#[async_trait]
pub trait ChatSwipeSource: Send + Sync {
    fn projection(self: Arc<Self>, source_id: u32) -> Box<dyn ChatByteReader>;
    async fn record(self: Arc<Self>, record: usize)
    -> Result<Box<dyn ChatByteReader>, DomainError>;
    /// Adapter-internal expansion of a staged payload; application services do not call this
    /// operation or handle its file paths.
    async fn restore_payload(
        self: Arc<Self>,
        source_id: u32,
        input: &Path,
        output: &Path,
        hash: bool,
    ) -> Result<RestoredChatPayload, DomainError>;
}

#[derive(Clone)]
pub struct ColdSwipeCommitSource {
    pub id: u32,
    pub source: Arc<dyn ChatSwipeSource>,
}

pub struct RestoredChatPayload {
    pub size: u64,
    pub sha256: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChatPayloadTarget {
    Character {
        character_id: String,
        file_name: String,
    },
    Group {
        chat_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatPayloadCommitBegin {
    pub session_id: String,
    pub max_frame_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedChatPayload {
    pub target: ChatPayloadTarget,
    pub accepted_size: u64,
    pub size: u64,
}

/// Atomically publishes full chat payloads or metadata-only updates.
#[async_trait]
pub trait ChatPayloadCommitRepository: Send + Sync {
    async fn open_swipe_source(
        &self,
        target: ChatPayloadTarget,
    ) -> Result<Arc<dyn ChatSwipeSource>, DomainError>;
    /// Replaces `chat_metadata` on an existing chat, preserving all body bytes.
    /// The incoming integrity must match any identity already in the header.
    async fn commit_metadata(
        &self,
        target: ChatPayloadTarget,
        chat_metadata: Value,
    ) -> Result<(), DomainError>;

    async fn begin(
        &self,
        target: ChatPayloadTarget,
        force: bool,
        cold_source: Option<ColdSwipeCommitSource>,
    ) -> Result<ChatPayloadCommitBegin, DomainError>;

    async fn append(&self, session_id: &str, offset: u64, bytes: &[u8])
    -> Result<u64, DomainError>;

    async fn finish(
        &self,
        session_id: &str,
        expected_size: u64,
    ) -> Result<CommittedChatPayload, DomainError>;

    /// Aborting an absent or already-consumed session is a successful no-op.
    async fn abort(&self, session_id: &str) -> Result<(), DomainError>;
}
