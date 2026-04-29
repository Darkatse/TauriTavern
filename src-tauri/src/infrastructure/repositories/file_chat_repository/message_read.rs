use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_types::{
    ChatMessageReadItem, ChatMessageRole, ChatMessagesReadResult,
};
use crate::infrastructure::logging::logger;

use super::FileChatRepository;

impl FileChatRepository {
    pub(super) async fn read_character_chat_messages_internal(
        &self,
        character_name: &str,
        file_name: &str,
        indices: &[usize],
    ) -> Result<ChatMessagesReadResult, DomainError> {
        let path = self.get_chat_path(character_name, file_name);
        read_chat_messages_from_path(&path, indices).await
    }

    pub(super) async fn read_group_chat_messages_internal(
        &self,
        chat_id: &str,
        indices: &[usize],
    ) -> Result<ChatMessagesReadResult, DomainError> {
        let path = self.get_group_chat_path(chat_id);
        read_chat_messages_from_path(&path, indices).await
    }
}

async fn read_chat_messages_from_path(
    path: &Path,
    indices: &[usize],
) -> Result<ChatMessagesReadResult, DomainError> {
    let file = File::open(path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Chat payload not found: {}", path.display()))
        } else {
            logger::error(&format!("Failed to open chat payload: {}", error));
            DomainError::InternalError(format!(
                "Failed to open chat payload {}: {}",
                path.display(),
                error
            ))
        }
    })?;
    let target_indices = indices.iter().copied().collect::<HashSet<_>>();
    let mut lines = BufReader::new(file).lines();

    let Some(header) = lines.next_line().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read chat payload header {}: {}",
            path.display(),
            error
        ))
    })?
    else {
        return Err(DomainError::InvalidData(format!(
            "Chat payload is empty: {}",
            path.display()
        )));
    };
    parse_json_line(&header, path, 1)?;

    let mut messages = Vec::new();
    let mut index = 0_usize;
    while let Some(line) = lines.next_line().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read chat payload {}: {}",
            path.display(),
            error
        ))
    })? {
        if line.trim().is_empty() {
            return Err(DomainError::InvalidData(format!(
                "Chat payload contains a blank message line at JSONL line {} for {}",
                index + 2,
                path.display()
            )));
        }

        let value = parse_json_line(&line, path, index + 2)?;
        if target_indices.contains(&index) {
            messages.push(read_item_from_value(index, &value)?);
        }
        index += 1;
    }

    Ok(ChatMessagesReadResult {
        total_messages: index,
        messages,
    })
}

fn parse_json_line(line: &str, path: &Path, line_number: usize) -> Result<Value, DomainError> {
    serde_json::from_str::<Value>(line).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to parse chat payload JSON at line {} for {}: {}",
            line_number,
            path.display(),
            error
        ))
    })
}

fn read_item_from_value(index: usize, value: &Value) -> Result<ChatMessageReadItem, DomainError> {
    let text = value
        .get("mes")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DomainError::InvalidData(format!("Chat message {index} has no string `mes` field"))
        })?
        .to_string();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let send_date = value
        .get("send_date")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Ok(ChatMessageReadItem {
        index,
        role: role_from_message_value(value),
        name,
        send_date,
        text,
    })
}

fn role_from_message_value(value: &Value) -> ChatMessageRole {
    if value
        .get("is_user")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        ChatMessageRole::User
    } else if value
        .get("is_system")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        ChatMessageRole::System
    } else {
        ChatMessageRole::Assistant
    }
}
