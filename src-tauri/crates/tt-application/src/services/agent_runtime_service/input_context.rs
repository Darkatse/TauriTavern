use serde_json::Value;

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use tt_domain::models::agent::AgentChatRef;
use tt_ports::repositories::chat_repository::{FindLastMessageQuery, LocatedChatMessage};

pub(super) struct AgentRunInputContext {
    pub(super) input_message_count: usize,
    pub(super) persist_base_state_id: Option<String>,
}

impl AgentRuntimeService {
    pub(super) async fn resolve_agent_run_input_context(
        &self,
        chat_ref: &AgentChatRef,
        generation_type: &str,
    ) -> Result<AgentRunInputContext, ApplicationError> {
        let last_message = self.find_last_chat_message(chat_ref).await?;
        let raw_message_count = last_message
            .as_ref()
            .map(|message| message.index.saturating_add(1))
            .unwrap_or(0);
        let input_message_count =
            resolve_input_message_count(generation_type, last_message.as_ref())?;
        let persist_base_state_id = self
            .resolve_persist_base_state_id(chat_ref, raw_message_count, input_message_count)
            .await?;

        Ok(AgentRunInputContext {
            input_message_count,
            persist_base_state_id,
        })
    }

    async fn find_last_chat_message(
        &self,
        chat_ref: &AgentChatRef,
    ) -> Result<Option<LocatedChatMessage>, ApplicationError> {
        let query = FindLastMessageQuery {
            role: None,
            has_top_level_keys: None,
            has_extra_keys: None,
            scan_limit: Some(1),
        };
        let result = match chat_ref {
            AgentChatRef::Character {
                character_id,
                file_name,
            } => {
                self.chat_repository
                    .find_last_character_chat_message(character_id, file_name, query)
                    .await
            }
            AgentChatRef::Group { chat_id } => {
                self.group_chat_repository
                    .find_last_group_chat_message(chat_id, query)
                    .await
            }
        };
        result.map_err(ApplicationError::from)
    }

    async fn resolve_persist_base_state_id(
        &self,
        chat_ref: &AgentChatRef,
        raw_message_count: usize,
        input_message_count: usize,
    ) -> Result<Option<String>, ApplicationError> {
        if input_message_count == 0 {
            return Ok(None);
        }

        let lines = match chat_ref {
            AgentChatRef::Character {
                character_id,
                file_name,
            } => {
                self.chat_repository
                    .get_chat_payload_tail_lines(character_id, file_name, raw_message_count)
                    .await?
                    .lines
            }
            AgentChatRef::Group { chat_id } => {
                self.group_chat_repository
                    .get_group_chat_payload_tail_lines(chat_id, raw_message_count)
                    .await?
                    .lines
            }
        };

        let visible_lines = lines.get(..input_message_count).ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.input_history_conflict: run input requires {input_message_count} messages, but chat payload only returned {}",
                lines.len()
            ))
        })?;

        for (index, line) in visible_lines.iter().enumerate().rev() {
            let message = serde_json::from_str::<Value>(line).map_err(|error| {
                ApplicationError::ValidationError(format!(
                    "agent.chat_message_invalid_json: failed to parse chat message {index}: {error}"
                ))
            })?;
            if let Some(state_id) = persist_state_id_from_message(&message, index)? {
                return Ok(Some(state_id));
            }
        }

        Ok(None)
    }
}

fn resolve_input_message_count(
    generation_type: &str,
    last_message: Option<&LocatedChatMessage>,
) -> Result<usize, ApplicationError> {
    let Some(last_message) = last_message else {
        return Ok(0);
    };
    let total_messages = last_message.index.saturating_add(1);

    match generation_type {
        "swipe" => {
            if !is_assistant_message(&last_message.message) {
                return Err(ApplicationError::ValidationError(
                    "agent.swipe_target_invalid: swipe generation requires the last chat message to be an assistant message"
                        .to_string(),
                ));
            }
            Ok(total_messages - 1)
        }
        "regenerate" if !is_user_message(&last_message.message) => Ok(total_messages - 1),
        _ => Ok(total_messages),
    }
}

fn persist_state_id_from_message(
    message: &Value,
    index: usize,
) -> Result<Option<String>, ApplicationError> {
    if !is_assistant_message(message) {
        return Ok(None);
    }

    let Some(metadata) = message
        .pointer("/extra/tauritavern/agent")
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };

    let status = metadata
        .get("persistStateStatus")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if status == "not_committed" {
        return Ok(None);
    }
    if !status.is_empty() && status != "committed" {
        return Err(ApplicationError::ValidationError(format!(
            "agent.persist_state_status_invalid: Agent result at chat message {index} has invalid persistStateStatus"
        )));
    }

    let state_id = metadata
        .get("persistStateId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.persist_state_missing: Agent result at chat message {index} has no persistStateId"
            ))
        })?;

    Ok(Some(state_id.to_string()))
}

fn is_user_message(message: &Value) -> bool {
    message
        .get("is_user")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn is_system_message(message: &Value) -> bool {
    message
        .get("is_system")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn is_assistant_message(message: &Value) -> bool {
    !is_user_message(message) && !is_system_message(message)
}
