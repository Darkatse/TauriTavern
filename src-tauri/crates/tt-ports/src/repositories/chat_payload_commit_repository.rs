use async_trait::async_trait;
use tt_domain::errors::DomainError;

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
    pub size: u64,
}

/// Streams a complete chat payload into private, target-volume staging and
/// publishes it atomically when the session is finished.
#[async_trait]
pub trait ChatPayloadCommitRepository: Send + Sync {
    async fn begin(
        &self,
        target: ChatPayloadTarget,
        force: bool,
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
