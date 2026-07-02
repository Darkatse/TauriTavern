mod read_messages;
mod search;
mod specs;

pub(super) use read_messages::read_messages;
pub(super) use search::search;
pub(super) use specs::{chat_read_messages_spec, chat_search_spec};

use crate::errors::ApplicationError;
use tt_domain::models::agent::{AgentChatRef, AgentRun};
use tt_ports::repositories::chat_repository::ChatMessageRole;
use tt_ports::repositories::chat_repository::{ChatRepository, FindLastMessageQuery};
use tt_ports::repositories::group_chat_repository::GroupChatRepository;

pub(super) const CHAT_READ_MESSAGES: &str = "chat.read_messages";
pub(super) const CHAT_SEARCH: &str = "chat.search";

const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_SCAN_LIMIT: usize = 100_000;
const MAX_MESSAGES_PER_READ: usize = 20;
const MAX_FULL_MESSAGE_CHARS: usize = 8_000;
const MAX_MESSAGE_RANGE_CHARS: usize = 8_000;
const MAX_TOTAL_READ_CHARS: usize = 20_000;

fn role_as_str(role: ChatMessageRole) -> &'static str {
    match role {
        ChatMessageRole::User => "user",
        ChatMessageRole::Assistant => "assistant",
        ChatMessageRole::System => "system",
    }
}

fn parse_role(value: &str) -> Option<ChatMessageRole> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Some(ChatMessageRole::User),
        "assistant" => Some(ChatMessageRole::Assistant),
        "system" => Some(ChatMessageRole::System),
        _ => None,
    }
}

async fn raw_total_messages(
    chat_repository: &dyn ChatRepository,
    group_chat_repository: &dyn GroupChatRepository,
    chat_ref: &AgentChatRef,
) -> Result<usize, ApplicationError> {
    let query = FindLastMessageQuery {
        role: None,
        has_top_level_keys: None,
        has_extra_keys: None,
        scan_limit: Some(1),
    };
    let last = match chat_ref {
        AgentChatRef::Character {
            character_id,
            file_name,
        } => {
            chat_repository
                .find_last_character_chat_message(character_id, file_name, query)
                .await
        }
        AgentChatRef::Group { chat_id } => {
            group_chat_repository
                .find_last_group_chat_message(chat_id, query)
                .await
        }
    }?;

    Ok(last
        .map(|message| message.index.saturating_add(1))
        .unwrap_or(0))
}

fn visible_total_messages(
    run: &AgentRun,
    raw_total_messages: usize,
) -> Result<usize, ApplicationError> {
    match run.input_message_count {
        Some(input_message_count) if raw_total_messages < input_message_count => {
            Err(ApplicationError::ValidationError(format!(
                "agent.input_history_conflict: run input requires {input_message_count} messages, but chat payload has {raw_total_messages}"
            )))
        }
        Some(input_message_count) => Ok(input_message_count),
        None => Ok(raw_total_messages),
    }
}
