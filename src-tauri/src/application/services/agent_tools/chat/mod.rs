mod read_messages;
mod search;
mod specs;

pub(super) use read_messages::read_messages;
pub(super) use search::search;
pub(super) use specs::{chat_read_messages_spec, chat_search_spec};

use crate::domain::repositories::chat_repository::ChatMessageRole;

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
