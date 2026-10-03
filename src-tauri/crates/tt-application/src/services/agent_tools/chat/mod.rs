mod descriptors;
mod read_messages;
mod search;

pub(super) use descriptors::{chat_read_messages_descriptor, chat_search_descriptor};
pub(super) use read_messages::read_messages;
pub(super) use search::search;

use crate::errors::ApplicationError;
use crate::services::agent_tools::common::tool_error;
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;

pub(super) const CHAT_READ_MESSAGES: &str = "chat.read_messages";
pub(super) const CHAT_SEARCH: &str = "chat.search";

const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_SCAN_LIMIT: usize = 100_000;
const MAX_MESSAGES_PER_READ: usize = 20;
const MAX_MESSAGE_READ_LINES: usize = 1_200;
const MAX_MESSAGE_READ_CHARS: usize = 8_000;
const MAX_TOTAL_READ_CHARS: usize = 20_000;

/// A chat that is gone is answered to the Agent, which can continue without it; other
/// read failures fail the call.
fn chat_read_error(
    call: &ToolInvocation,
    error: DomainError,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    match error {
        DomainError::NotFound(message) => Ok((
            tool_error(
                call,
                "chat.not_found",
                &format!(
                    "{message}\n\nThe current chat is no longer available. Continue with the context already present in this run. If you need the missing history, ask the user to retry from an available chat."
                ),
            ),
            AgentToolEffect::None,
        )),
        error => Err(error.into()),
    }
}
