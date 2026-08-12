mod descriptors;
mod read_messages;
mod search;

pub(super) use descriptors::{chat_read_messages_descriptor, chat_search_descriptor};
pub(super) use read_messages::read_messages;
pub(super) use search::search;

use crate::errors::ApplicationError;
use tt_domain::errors::DomainError;
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
const MAX_MESSAGE_READ_LINES: usize = 1_200;
const MAX_MESSAGE_READ_CHARS: usize = 8_000;
const MAX_TOTAL_READ_CHARS: usize = 20_000;

fn role_as_str(role: ChatMessageRole) -> &'static str {
    match role {
        ChatMessageRole::User => "user",
        ChatMessageRole::Assistant => "assistant",
        ChatMessageRole::System => "system",
        ChatMessageRole::Tool => "tool",
    }
}

fn parse_role(value: &str) -> Option<ChatMessageRole> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Some(ChatMessageRole::User),
        "assistant" => Some(ChatMessageRole::Assistant),
        "system" => Some(ChatMessageRole::System),
        "tool" => Some(ChatMessageRole::Tool),
        _ => None,
    }
}

async fn raw_total_messages(
    chat_repository: &dyn ChatRepository,
    group_chat_repository: &dyn GroupChatRepository,
    chat_ref: &AgentChatRef,
) -> Result<usize, DomainError> {
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

fn chat_unavailable_message(message: &str) -> String {
    format!(
        "{message}\n\nThe current chat is no longer available. Continue with the context already present in this run. If you need the missing history, ask the user to retry from an available chat."
    )
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

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{chat_search_descriptor, parse_role, role_as_str};
    use tt_ports::repositories::chat_repository::ChatMessageRole;

    #[test]
    fn tool_role_is_supported_by_chat_search() {
        assert_eq!(parse_role("tool"), Some(ChatMessageRole::Tool));
        assert_eq!(role_as_str(ChatMessageRole::Tool), "tool");

        let descriptor = chat_search_descriptor();
        let roles = descriptor
            .input_schema
            .pointer("/properties/role/enum")
            .and_then(Value::as_array)
            .expect("chat search role enum");
        assert!(roles.iter().any(|role| role.as_str() == Some("tool")));
    }
}
