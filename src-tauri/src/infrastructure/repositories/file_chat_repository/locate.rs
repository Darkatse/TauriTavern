use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_repository::{
    ChatMessageRole, FindLastMessageQuery, LocatedChatMessage,
};

use super::FileChatRepository;

fn parse_message_line(line: &str) -> Result<Value, DomainError> {
    serde_json::from_str::<Value>(line).map_err(|error| {
        DomainError::InvalidData(format!("Failed to parse chat message JSON: {}", error))
    })
}

fn matches_role(message: &Value, role: ChatMessageRole) -> bool {
    let is_user = message
        .get("is_user")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_system = message
        .get("is_system")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    match role {
        ChatMessageRole::User => is_user,
        ChatMessageRole::System => is_system,
        ChatMessageRole::Assistant => !is_user && !is_system,
    }
}

fn has_top_level_keys(message: &Value, keys: &[String]) -> bool {
    keys.iter().all(|key| message.get(key).is_some())
}

fn has_extra_keys(message: &Value, keys: &[String]) -> bool {
    let extra = message.get("extra").and_then(Value::as_object);
    match extra {
        Some(extra) => keys.iter().all(|key| extra.get(key).is_some()),
        None => false,
    }
}

fn resolve_scan_limit(query: &FindLastMessageQuery) -> Result<usize, DomainError> {
    let scan_limit = query.scan_limit.unwrap_or(2000);
    if scan_limit == 0 {
        return Err(DomainError::InvalidData(
            "scanLimit must be greater than 0".to_string(),
        ));
    }
    Ok(scan_limit)
}

impl FileChatRepository {
    pub(super) async fn find_last_character_chat_message_internal(
        &self,
        character_name: &str,
        file_name: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, DomainError> {
        let summary = self
            .get_character_chat_summary_internal(character_name, file_name, false)
            .await?;
        let total_count = summary.message_count;
        if total_count == 0 {
            return Ok(None);
        }

        let scan_limit = resolve_scan_limit(&query)?;
        let tail = self
            .get_character_payload_tail_lines(character_name, file_name, scan_limit)
            .await?;
        let lines = tail.lines;
        if lines.is_empty() {
            return Ok(None);
        }

        let required_top_level = query.has_top_level_keys.unwrap_or_default();
        let required_extra = query.has_extra_keys.unwrap_or_default();

        for (from_end, line) in lines.iter().rev().enumerate() {
            let message = parse_message_line(line)?;

            if let Some(role) = query.role {
                if !matches_role(&message, role) {
                    continue;
                }
            }

            if !required_top_level.is_empty() && !has_top_level_keys(&message, &required_top_level)
            {
                continue;
            }

            if !required_extra.is_empty() && !has_extra_keys(&message, &required_extra) {
                continue;
            }

            let index = total_count
                .saturating_sub(1)
                .saturating_sub(from_end);
            return Ok(Some(LocatedChatMessage { index, message }));
        }

        Ok(None)
    }

    pub(super) async fn find_last_group_chat_message_internal(
        &self,
        chat_id: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, DomainError> {
        let summary = self.get_group_chat_summary_internal(chat_id, false).await?;
        let total_count = summary.message_count;
        if total_count == 0 {
            return Ok(None);
        }

        let scan_limit = resolve_scan_limit(&query)?;
        let tail = self.get_group_payload_tail_lines(chat_id, scan_limit).await?;
        let lines = tail.lines;
        if lines.is_empty() {
            return Ok(None);
        }

        let required_top_level = query.has_top_level_keys.unwrap_or_default();
        let required_extra = query.has_extra_keys.unwrap_or_default();

        for (from_end, line) in lines.iter().rev().enumerate() {
            let message = parse_message_line(line)?;

            if let Some(role) = query.role {
                if !matches_role(&message, role) {
                    continue;
                }
            }

            if !required_top_level.is_empty() && !has_top_level_keys(&message, &required_top_level)
            {
                continue;
            }

            if !required_extra.is_empty() && !has_extra_keys(&message, &required_extra) {
                continue;
            }

            let index = total_count
                .saturating_sub(1)
                .saturating_sub(from_end);
            return Ok(Some(LocatedChatMessage { index, message }));
        }

        Ok(None)
    }
}

