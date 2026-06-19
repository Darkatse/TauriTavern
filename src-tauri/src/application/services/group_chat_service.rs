use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use crate::application::dto::chat_dto::{
    ChatSearchResultDto, DeleteGroupChatDto, ImportGroupChatDto, RenameGroupChatDto,
    SaveGroupChatFromFileDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_workspace_lifecycle_service::AgentWorkspaceLifecycleService;
use crate::application::services::chat_file_validation::validate_chat_file_name;
use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_types::{
    ChatMessageSearchHit, ChatMessageSearchQuery, ChatPayloadChunk, ChatPayloadCursor,
    ChatPayloadPatchOp, ChatPayloadTail, FindLastMessageQuery, LocatedChatMessage, PinnedGroupChat,
};
use crate::domain::repositories::group_chat_repository::GroupChatRepository;

/// Service for managing group chats (JSONL payloads).
pub struct GroupChatService {
    group_chat_repository: Arc<dyn GroupChatRepository>,
    agent_workspace_lifecycle_service: Arc<AgentWorkspaceLifecycleService>,
}

impl GroupChatService {
    pub fn new(
        group_chat_repository: Arc<dyn GroupChatRepository>,
        agent_workspace_lifecycle_service: Arc<AgentWorkspaceLifecycleService>,
    ) -> Self {
        Self {
            group_chat_repository,
            agent_workspace_lifecycle_service,
        }
    }

    /// List group chat summaries without loading full chat payloads.
    pub async fn list_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        let results = self
            .group_chat_repository
            .list_group_chat_summaries(chat_ids, include_metadata)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// List recent group chat summaries without full summary scan.
    pub async fn list_recent_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
        max_entries: usize,
        pinned: &[PinnedGroupChat],
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        let results = self
            .group_chat_repository
            .list_recent_group_chat_summaries(chat_ids, include_metadata, max_entries, pinned)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// Search group chats with optional chat ID filtering.
    pub async fn search_group_chats(
        &self,
        query: &str,
        chat_ids: Option<&[String]>,
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        let results = self
            .group_chat_repository
            .search_group_chats(query, chat_ids)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    pub async fn get_group_chat_summary(
        &self,
        chat_id: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResultDto, ApplicationError> {
        let summary = self
            .group_chat_repository
            .get_group_chat_summary(chat_id, include_metadata)
            .await?;
        Ok(ChatSearchResultDto::from(summary))
    }

    pub async fn get_group_chat_metadata(&self, chat_id: &str) -> Result<Value, ApplicationError> {
        Ok(self
            .group_chat_repository
            .get_group_chat_metadata(chat_id)
            .await?)
    }

    pub async fn set_group_chat_metadata_extension(
        &self,
        chat_id: &str,
        namespace: &str,
        value: Value,
    ) -> Result<(), ApplicationError> {
        self.group_chat_repository
            .set_group_chat_metadata_extension(chat_id, namespace, value)
            .await?;
        Ok(())
    }

    pub async fn get_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Value, ApplicationError> {
        Ok(self
            .group_chat_repository
            .get_group_chat_store_json(chat_id, namespace, key)
            .await?)
    }

    pub async fn set_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), ApplicationError> {
        self.group_chat_repository
            .set_group_chat_store_json(chat_id, namespace, key, value)
            .await?;
        Ok(())
    }

    pub async fn update_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), ApplicationError> {
        self.group_chat_repository
            .update_group_chat_store_json(chat_id, namespace, key, value)
            .await?;
        Ok(())
    }

    pub async fn rename_group_chat_store_key(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), ApplicationError> {
        self.group_chat_repository
            .rename_group_chat_store_key(chat_id, namespace, key, new_key)
            .await?;
        Ok(())
    }

    pub async fn delete_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), ApplicationError> {
        self.group_chat_repository
            .delete_group_chat_store_json(chat_id, namespace, key)
            .await?;
        Ok(())
    }

    pub async fn list_group_chat_store_keys(
        &self,
        chat_id: &str,
        namespace: &str,
    ) -> Result<Vec<String>, ApplicationError> {
        Ok(self
            .group_chat_repository
            .list_group_chat_store_keys(chat_id, namespace)
            .await?)
    }

    pub async fn find_last_group_chat_message(
        &self,
        chat_id: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, ApplicationError> {
        Ok(self
            .group_chat_repository
            .find_last_group_chat_message(chat_id, query)
            .await?)
    }

    pub async fn search_group_chat_messages(
        &self,
        chat_id: &str,
        query: ChatMessageSearchQuery,
    ) -> Result<Vec<ChatMessageSearchHit>, ApplicationError> {
        Ok(self
            .group_chat_repository
            .search_group_chat_messages(chat_id, query)
            .await?)
    }

    /// Clear the group chat cache.
    pub async fn clear_cache(&self) -> Result<(), DomainError> {
        self.group_chat_repository.clear_cache().await
    }

    /// Get the absolute path to a group chat payload file.
    pub async fn get_group_chat_payload_path(
        &self,
        chat_id: &str,
    ) -> Result<String, ApplicationError> {
        if chat_id.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Group chat id cannot be empty".to_string(),
            ));
        }

        let path = self
            .group_chat_repository
            .get_group_chat_payload_path(chat_id)
            .await?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Get the tail window for a group chat JSONL payload.
    pub async fn get_group_chat_payload_tail_lines(
        &self,
        chat_id: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, ApplicationError> {
        self.group_chat_repository
            .get_group_chat_payload_tail_lines(chat_id, max_lines)
            .await
            .map_err(Into::into)
    }

    /// Get JSONL lines before the current group chat window cursor.
    pub async fn get_group_chat_payload_before_lines(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, ApplicationError> {
        self.group_chat_repository
            .get_group_chat_payload_before_lines(chat_id, cursor, max_lines)
            .await
            .map_err(Into::into)
    }

    /// Get multiple windows of JSONL lines before the current group chat window cursor.
    ///
    /// This is equivalent to calling `get_group_chat_payload_before_lines` repeatedly, but returns
    /// multiple pages in one IPC round-trip.
    pub async fn get_group_chat_payload_before_pages_lines(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
        max_pages: usize,
    ) -> Result<Vec<ChatPayloadChunk>, ApplicationError> {
        if max_lines == 0 || max_pages == 0 {
            return Err(ApplicationError::ValidationError(
                "max_lines and max_pages must be greater than 0".to_string(),
            ));
        }

        let mut pages = Vec::with_capacity(max_pages);
        let mut next_cursor = cursor;

        for _ in 0..max_pages {
            let page = self
                .group_chat_repository
                .get_group_chat_payload_before_lines(chat_id, next_cursor, max_lines)
                .await?;

            next_cursor = page.cursor;
            let done = page.lines.is_empty() || !page.has_more_before;
            pages.push(page);

            if done {
                break;
            }
        }

        Ok(pages)
    }

    /// Save a windowed group chat payload by preserving bytes before cursor.offset and
    /// overwriting from cursor.offset using the provided JSONL lines.
    pub async fn save_group_chat_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        lines: Vec<String>,
        expected_window_line_count: usize,
        force: bool,
    ) -> Result<ChatPayloadCursor, ApplicationError> {
        validate_chat_file_name(chat_id, "Group chat id")?;

        self.group_chat_repository
            .save_group_chat_payload_windowed(
                chat_id,
                cursor,
                header,
                lines,
                expected_window_line_count,
                force,
            )
            .await
            .map_err(Into::into)
    }

    /// Patch a windowed group chat payload.
    pub async fn patch_group_chat_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        op: ChatPayloadPatchOp,
        expected_window_line_count: usize,
        force: bool,
    ) -> Result<ChatPayloadCursor, ApplicationError> {
        validate_chat_file_name(chat_id, "Group chat id")?;

        self.group_chat_repository
            .patch_group_chat_payload_windowed(
                chat_id,
                cursor,
                header,
                op,
                expected_window_line_count,
                force,
            )
            .await
            .map_err(Into::into)
    }

    /// Set the hidden flag on all messages stored before the window cursor.
    pub async fn hide_group_chat_payload_before_cursor(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        hide: bool,
        name_filter: Option<String>,
        expected_window_line_count: usize,
    ) -> Result<ChatPayloadCursor, ApplicationError> {
        validate_chat_file_name(chat_id, "Group chat id")?;

        self.group_chat_repository
            .hide_group_chat_payload_before_cursor(
                chat_id,
                cursor,
                hide,
                name_filter,
                expected_window_line_count,
            )
            .await
            .map_err(Into::into)
    }

    /// Save a group chat payload from a JSONL file path.
    pub async fn save_group_chat_from_file(
        &self,
        dto: SaveGroupChatFromFileDto,
    ) -> Result<(), ApplicationError> {
        validate_chat_file_name(&dto.id, "Group chat id")?;

        self.group_chat_repository
            .save_group_chat_payload_from_path(
                &dto.id,
                Path::new(&dto.file_path),
                dto.force.unwrap_or(false),
            )
            .await
            .map_err(Into::into)
    }

    /// Delete a group chat payload file.
    pub async fn delete_group_chat(&self, dto: DeleteGroupChatDto) -> Result<(), ApplicationError> {
        validate_chat_file_name(&dto.id, "Group chat id")?;

        let target = AgentWorkspaceLifecycleService::group_target(&dto.id)?;
        self.agent_workspace_lifecycle_service
            .ensure_chat_workspace_inactive(&target)
            .await?;

        self.group_chat_repository
            .delete_group_chat_payload(&dto.id)
            .await?;
        self.agent_workspace_lifecycle_service
            .delete_chat_workspace(&target)
            .await?;
        Ok(())
    }

    /// Rename a group chat payload file.
    pub async fn rename_group_chat(
        &self,
        dto: RenameGroupChatDto,
    ) -> Result<String, ApplicationError> {
        validate_chat_file_name(&dto.old_file_name, "Old group chat file name")?;
        validate_chat_file_name(&dto.new_file_name, "New group chat file name")?;

        let committed_file_name = self
            .group_chat_repository
            .rename_group_chat_payload(&dto.old_file_name, &dto.new_file_name)
            .await?;
        Ok(committed_file_name)
    }

    /// Import a group chat payload and return the created chat id.
    pub async fn import_group_chat(
        &self,
        dto: ImportGroupChatDto,
    ) -> Result<String, ApplicationError> {
        self.group_chat_repository
            .import_group_chat_payload(Path::new(&dto.file_path))
            .await
            .map_err(Into::into)
    }
}
