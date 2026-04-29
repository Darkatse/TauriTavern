use serde_json::json;
use sha2::{Digest, Sha256};

use crate::application::errors::ApplicationError;
use crate::domain::models::agent::AgentChatRef;

pub(super) fn workspace_id_for_stable_chat_id(
    chat_ref: &AgentChatRef,
    stable_chat_id: &str,
) -> Result<String, ApplicationError> {
    let kind = match chat_ref {
        AgentChatRef::Character { .. } => "character",
        AgentChatRef::Group { .. } => "group",
    };
    let json = serde_json::to_vec(&json!({
        "kind": kind,
        "stableChatId": stable_chat_id,
    }))
    .map_err(|error| {
        ApplicationError::ValidationError(format!("agent.invalid_chat_ref: {error}"))
    })?;
    let digest = Sha256::digest(json);
    let mut suffix = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        suffix.push_str(&format!("{byte:02x}"));
    }
    Ok(format!("chat_{suffix}"))
}

pub(super) fn validate_stable_chat_id(raw: &str) -> Result<String, ApplicationError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.stable_chat_id_required: stableChatId is required".to_string(),
        ));
    }
    if value.len() > 512 {
        return Err(ApplicationError::ValidationError(
            "agent.invalid_stable_chat_id: stableChatId is too long".to_string(),
        ));
    }
    Ok(value.to_string())
}

pub(super) fn safe_workspace_file_stem(value: &str) -> String {
    let mut output = String::with_capacity(value.len().max(1));
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' {
            output.push(byte as char);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "tool_call".to_string()
    } else {
        output
    }
}
