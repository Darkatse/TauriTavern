mod descriptors;
mod message_source;
mod read_messages;
mod search;

pub(super) use descriptors::{chat_read_messages_descriptor, chat_search_descriptor};
pub(super) use message_source::CharacterChatMessageSource;
pub(super) use read_messages::read_messages;
pub(super) use search::search;

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

/// Failure every chat reader reports when a run carries no frozen input count.
///
/// The count is the only trustworthy upper bound for the run's history, so a reader
/// without it has nothing to bound a read by. Every run the application creates has
/// it (see `AgentRunInputContext`), so a missing one means the record is damaged or
/// predates the field; falling back to the live chat length would expose messages the
/// run was not built from. The marker is what lets a caller tell that state apart
/// from `chat.unsupported`, where the capability itself is absent.
fn chat_input_count_missing(run: &AgentRun) -> DomainError {
    DomainError::InvalidData(format!(
        "agent.chat_input_count_missing: run `{}` has no frozen input message count",
        run.id
    ))
}

/// Upper bound of the run's frozen input history, or a failure when that bound cannot
/// be established.
///
/// Reported as a [`DomainError`] so every caller keeps the same message: tool callers
/// convert it through `From<DomainError> for ApplicationError`, and the script-facing
/// port returns it unchanged instead of re-wrapping it and burying the `agent.*`
/// marker under an extra type prefix.
fn visible_total_messages(run: &AgentRun, raw_total_messages: usize) -> Result<usize, DomainError> {
    let Some(input_message_count) = run.chat_target()?.input_message_count else {
        return Err(chat_input_count_missing(run));
    };
    if raw_total_messages < input_message_count {
        return Err(DomainError::InvalidData(format!(
            "agent.input_history_conflict: run input requires {input_message_count} messages, but chat payload has {raw_total_messages}"
        )));
    }
    Ok(input_message_count)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::Value;

    use super::{chat_search_descriptor, parse_role, role_as_str, visible_total_messages};
    use tt_domain::models::agent::{
        AgentChatRef, AgentChatRunTarget, AgentRun, AgentRunPresentation, AgentRunSkillScopeRefs,
        AgentRunStatus, AgentRunTarget,
    };
    use tt_ports::repositories::chat_repository::ChatMessageRole;

    /// A Chat run whose frozen input count the caller chooses.
    ///
    /// `None` models a record that predates the field or was damaged; every run the
    /// application creates carries a count, so this is what the fail-fast path is for.
    fn chat_run(input_message_count: Option<usize>) -> AgentRun {
        AgentRun {
            id: "run_visible_total".to_string(),
            workspace_id: "workspace_visible_total".to_string(),
            target: AgentRunTarget::Chat(AgentChatRunTarget {
                stable_chat_id: "stable_visible_total".to_string(),
                chat_ref: AgentChatRef::Character {
                    character_id: "Alice".to_string(),
                    file_name: "Alice.jsonl".to_string(),
                },
                generation_type: "normal".to_string(),
                skill_scope_refs: AgentRunSkillScopeRefs::default(),
                persist_base_state_id: None,
                input_message_count,
                presentation: AgentRunPresentation::Background,
            }),
            profile_id: None,
            status: AgentRunStatus::Created,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_run_without_a_frozen_input_count_fails_every_chat_read() {
        // The frozen count is the only trustworthy bound for the run's own history,
        // so a reader without one fails instead of falling back to the live chat
        // length and exposing messages the run was not built from. The marker is
        // what lets a caller tell that state apart from `chat.unsupported`, where
        // the capability itself is absent.
        let error = visible_total_messages(&chat_run(None), 12)
            .expect_err("a run without a frozen count cannot be bounded");
        assert!(
            error.to_string().contains("agent.chat_input_count_missing"),
            "{error}"
        );
        assert!(error.to_string().contains("run_visible_total"), "{error}");
    }

    #[test]
    fn the_frozen_input_count_bounds_the_visible_history() {
        // A payload shorter than the frozen count means history the run was built
        // from is gone; a longer one is normal, because the chat keeps growing after
        // the run started. The frozen count is the bound either way.
        let conflict = visible_total_messages(&chat_run(Some(12)), 11)
            .expect_err("a payload shorter than the frozen count is a conflict");
        assert!(
            conflict
                .to_string()
                .contains("agent.input_history_conflict"),
            "{conflict}"
        );

        for raw_total in [12, 40] {
            let visible = visible_total_messages(&chat_run(Some(12)), raw_total)
                .expect("a payload at or past the frozen count is fine");
            assert_eq!(visible, 12, "raw total {raw_total}");
        }
    }

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
