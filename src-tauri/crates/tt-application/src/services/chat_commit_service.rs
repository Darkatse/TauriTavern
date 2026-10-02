use std::sync::Arc;

use tt_contracts::byte_commit::CommitBegin;
use tt_ports::repositories::chat_commit_repository::{
    ChatCommitOperation, ChatCommitRepository, ChatCommitTarget, ChatSwipeSource,
};

use crate::dto::chat_history_dto::{ChatHistoryLocator, CurrentCommitReason};
use crate::errors::ApplicationError;
use crate::services::chat_file_validation::validate_chat_history_locator;
use crate::services::chat_history_coordinator::ChatHistoryCoordinator;

/// Coordinates full-payload and metadata commits with chat-history scheduling.
pub struct ChatCommitService {
    repository: Arc<dyn ChatCommitRepository>,
    chat_history_coordinator: Arc<ChatHistoryCoordinator>,
}

impl ChatCommitService {
    pub fn new(
        repository: Arc<dyn ChatCommitRepository>,
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
        operation: ChatCommitOperation,
    ) -> Result<CommitBegin, ApplicationError> {
        validate_chat_history_locator(&locator)?;
        if let ChatCommitOperation::MetadataExtension { namespace } = &operation
            && namespace.trim().is_empty()
        {
            return Err(ApplicationError::ValidationError(
                "namespace is required".into(),
            ));
        }
        Ok(self
            .repository
            .begin(target_from_locator(locator), operation)
            .await?)
    }

    pub async fn open_swipe_source(
        &self,
        locator: ChatHistoryLocator,
        allow_not_found: bool,
    ) -> Result<Option<Arc<dyn ChatSwipeSource>>, ApplicationError> {
        validate_chat_history_locator(&locator)?;
        match self
            .repository
            .open_swipe_source(target_from_locator(locator))
            .await
        {
            Ok(source) => Ok(Some(source)),
            Err(tt_domain::errors::DomainError::NotFound(_)) if allow_not_found => Ok(None),
            Err(error) => Err(error.into()),
        }
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
    ) -> Result<(), ApplicationError> {
        let target = self.repository.finish(session_id, expected_size).await?;
        self.chat_history_coordinator
            .note_current_committed(target.into(), commit_reason)
            .await;
        Ok(())
    }

    pub async fn abort(&self, session_id: &str) -> Result<(), ApplicationError> {
        Ok(self.repository.abort(session_id).await?)
    }
}

fn target_from_locator(locator: ChatHistoryLocator) -> ChatCommitTarget {
    match locator {
        ChatHistoryLocator::Character {
            character_id,
            file_name,
        } => ChatCommitTarget::Character {
            character_id,
            file_name,
        },
        ChatHistoryLocator::Group { chat_id } => ChatCommitTarget::Group { chat_id },
    }
}

impl From<ChatCommitTarget> for ChatHistoryLocator {
    fn from(target: ChatCommitTarget) -> Self {
        match target {
            ChatCommitTarget::Character {
                character_id,
                file_name,
            } => Self::Character {
                character_id,
                file_name,
            },
            ChatCommitTarget::Group { chat_id } => Self::Group { chat_id },
        }
    }
}
