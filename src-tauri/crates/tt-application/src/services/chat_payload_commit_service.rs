use std::sync::Arc;

use tt_ports::repositories::chat_payload_commit_repository::{
    ChatPayloadCommitBegin, ChatPayloadCommitRepository, ChatPayloadTarget,
};

use crate::dto::chat_history_dto::{ChatHistoryLocator, CurrentCommitReason};
use crate::errors::ApplicationError;
use crate::services::chat_file_validation::validate_chat_history_locator;
use crate::services::chat_history_coordinator::ChatHistoryCoordinator;

/// Coordinates streamed full-payload commits with chat-history scheduling.
pub struct ChatPayloadCommitService {
    repository: Arc<dyn ChatPayloadCommitRepository>,
    chat_history_coordinator: Arc<ChatHistoryCoordinator>,
}

impl ChatPayloadCommitService {
    pub fn new(
        repository: Arc<dyn ChatPayloadCommitRepository>,
        chat_history_coordinator: Arc<ChatHistoryCoordinator>,
    ) -> Self {
        Self {
            repository,
            chat_history_coordinator,
        }
    }

    pub async fn begin(
        &self,
        locator: ChatHistoryLocator,
        force: bool,
    ) -> Result<ChatPayloadCommitBegin, ApplicationError> {
        validate_chat_history_locator(&locator)?;
        Ok(self
            .repository
            .begin(target_from_locator(locator), force)
            .await?)
    }

    pub async fn append(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, ApplicationError> {
        Ok(self.repository.append(session_id, offset, bytes).await?)
    }

    pub async fn finish(
        &self,
        session_id: &str,
        expected_size: u64,
        commit_reason: CurrentCommitReason,
    ) -> Result<u64, ApplicationError> {
        let committed = self.repository.finish(session_id, expected_size).await?;
        self.chat_history_coordinator
            .note_current_committed(committed.target.into(), commit_reason)
            .await;
        Ok(committed.size)
    }

    pub async fn abort(&self, session_id: &str) -> Result<(), ApplicationError> {
        Ok(self.repository.abort(session_id).await?)
    }
}

fn target_from_locator(locator: ChatHistoryLocator) -> ChatPayloadTarget {
    match locator {
        ChatHistoryLocator::Character {
            character_id,
            file_name,
        } => ChatPayloadTarget::Character {
            character_id,
            file_name,
        },
        ChatHistoryLocator::Group { chat_id } => ChatPayloadTarget::Group { chat_id },
    }
}

impl From<ChatPayloadTarget> for ChatHistoryLocator {
    fn from(target: ChatPayloadTarget) -> Self {
        match target {
            ChatPayloadTarget::Character {
                character_id,
                file_name,
            } => Self::Character {
                character_id,
                file_name,
            },
            ChatPayloadTarget::Group { chat_id } => Self::Group { chat_id },
        }
    }
}
