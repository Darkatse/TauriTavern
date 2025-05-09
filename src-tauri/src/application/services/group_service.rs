use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::errors::DomainError;
use crate::domain::models::group::Group;
use crate::domain::repositories::group_repository::GroupRepository;
use crate::application::dto::group_dto::{CreateGroupDto, UpdateGroupDto, DeleteGroupDto};
use crate::infrastructure::logging::logger;

/// Service for managing groups
pub struct GroupService {
    /// Repository for group data
    repository: Arc<dyn GroupRepository>,
}

impl GroupService {
    /// Create a new GroupService
    pub fn new(repository: Arc<dyn GroupRepository>) -> Self {
        Self { repository }
    }
    
    /// Get all groups
    pub async fn get_all_groups(&self) -> Result<Vec<Group>, DomainError> {
        logger::debug("GroupService: Getting all groups");
        self.repository.get_all_groups().await
    }
    
    /// Get a group by ID
    pub async fn get_group(&self, id: &str) -> Result<Option<Group>, DomainError> {
        logger::debug(&format!("GroupService: Getting group {}", id));
        self.repository.get_group(id).await
    }
    
    /// Create a new group
    pub async fn create_group(&self, dto: CreateGroupDto) -> Result<Group, DomainError> {
        logger::debug(&format!("GroupService: Creating group {}", dto.name));
        
        // Generate a unique ID based on timestamp
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis().to_string())
            .unwrap_or_else(|_| String::from("fallback_id"));
        
        // Use provided chat_id or generate one
        let chat_id = dto.chat_id.unwrap_or_else(|| id.clone());
        
        // Use provided chats or create a new list with the chat_id
        let chats = dto.chats.unwrap_or_else(|| vec![chat_id.clone()]);
        
        // Create the group model
        let group = Group {
            id,
            name: dto.name,
            members: dto.members,
            avatar_url: dto.avatar_url,
            allow_self_responses: dto.allow_self_responses,
            activation_strategy: dto.activation_strategy,
            generation_mode: dto.generation_mode,
            disabled_members: dto.disabled_members,
            chat_metadata: dto.chat_metadata,
            fav: dto.fav,
            chat_id,
            chats,
            auto_mode_delay: dto.auto_mode_delay.unwrap_or(5),
            generation_mode_join_prefix: dto.generation_mode_join_prefix.unwrap_or_default(),
            generation_mode_join_suffix: dto.generation_mode_join_suffix.unwrap_or_default(),
            hide_muted_sprites: dto.hide_muted_sprites.unwrap_or(true),
            past_metadata: Default::default(),
            date_added: None,
            create_date: None,
            chat_size: None,
            date_last_chat: None,
        };
        
        // Save the group
        self.repository.create_group(&group).await
    }
    
    /// Update an existing group
    pub async fn update_group(&self, dto: UpdateGroupDto) -> Result<Group, DomainError> {
        logger::debug(&format!("GroupService: Updating group {}", dto.id));
        
        // Get the existing group
        let existing_group = self.repository.get_group(&dto.id).await?
            .ok_or_else(|| DomainError::NotFound(format!("Group not found: {}", dto.id)))?;
        
        // Update the group with new values
        let updated_group = Group {
            id: existing_group.id,
            name: dto.name.unwrap_or(existing_group.name),
            members: dto.members.unwrap_or(existing_group.members),
            avatar_url: dto.avatar_url.or(existing_group.avatar_url),
            allow_self_responses: dto.allow_self_responses.unwrap_or(existing_group.allow_self_responses),
            activation_strategy: dto.activation_strategy.unwrap_or(existing_group.activation_strategy),
            generation_mode: dto.generation_mode.unwrap_or(existing_group.generation_mode),
            disabled_members: dto.disabled_members.unwrap_or(existing_group.disabled_members),
            chat_metadata: dto.chat_metadata.unwrap_or(existing_group.chat_metadata),
            fav: dto.fav.unwrap_or(existing_group.fav),
            chat_id: dto.chat_id.unwrap_or(existing_group.chat_id),
            chats: dto.chats.unwrap_or(existing_group.chats),
            auto_mode_delay: dto.auto_mode_delay.unwrap_or(existing_group.auto_mode_delay),
            generation_mode_join_prefix: dto.generation_mode_join_prefix.unwrap_or(existing_group.generation_mode_join_prefix),
            generation_mode_join_suffix: dto.generation_mode_join_suffix.unwrap_or(existing_group.generation_mode_join_suffix),
            hide_muted_sprites: dto.hide_muted_sprites.unwrap_or(existing_group.hide_muted_sprites),
            past_metadata: dto.past_metadata.unwrap_or(existing_group.past_metadata),
            date_added: existing_group.date_added,
            create_date: existing_group.create_date,
            chat_size: existing_group.chat_size,
            date_last_chat: existing_group.date_last_chat,
        };
        
        // Save the updated group
        self.repository.update_group(&updated_group).await
    }
    
    /// Delete a group
    pub async fn delete_group(&self, dto: DeleteGroupDto) -> Result<(), DomainError> {
        logger::debug(&format!("GroupService: Deleting group {}", dto.id));
        self.repository.delete_group(&dto.id).await
    }
    
    /// Get all group chat paths
    pub async fn get_group_chat_paths(&self) -> Result<Vec<String>, DomainError> {
        logger::debug("GroupService: Getting all group chat paths");
        self.repository.get_group_chat_paths().await
    }
    
    /// Clear the group cache
    pub async fn clear_cache(&self) -> Result<(), DomainError> {
        logger::debug("GroupService: Clearing group cache");
        self.repository.clear_cache().await
    }
}
