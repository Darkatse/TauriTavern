use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use super::{
    MAX_FULL_MESSAGE_CHARS, MAX_MESSAGE_RANGE_CHARS, MAX_MESSAGES_PER_READ, MAX_TOTAL_READ_CHARS,
    role_as_str,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::common::{object_args, tool_error};
use crate::application::services::agent_tools::dispatcher::AgentToolEffect;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentChatRef, AgentToolCall, AgentToolResult};
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::chat_repository::{ChatMessageReadItem, ChatRepository};
use crate::domain::repositories::group_chat_repository::GroupChatRepository;

#[derive(Debug, Clone)]
struct MessageRequest {
    index: usize,
    start_char: Option<usize>,
    max_chars: Option<usize>,
}

struct RenderedMessage {
    index: usize,
    role: &'static str,
    name: Option<String>,
    send_date: Option<String>,
    start_char: usize,
    end_char: usize,
    total_chars: usize,
    truncated: bool,
    text: String,
    ref_id: String,
}

pub(in crate::application::services::agent_tools) async fn read_messages(
    run_repository: &dyn AgentRunRepository,
    chat_repository: &dyn ChatRepository,
    group_chat_repository: &dyn GroupChatRepository,
    run_id: &str,
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let requests = match parse_message_requests(args) {
        Ok(requests) => requests,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    let run = run_repository.load_run(run_id).await?;
    let indices = requests
        .iter()
        .map(|request| request.index)
        .collect::<Vec<_>>();
    let read = match &run.chat_ref {
        AgentChatRef::Character {
            character_id,
            file_name,
        } => {
            chat_repository
                .read_character_chat_messages(character_id, file_name, &indices)
                .await
        }
        AgentChatRef::Group { chat_id } => {
            group_chat_repository
                .read_group_chat_messages(chat_id, &indices)
                .await
        }
    };
    let read = match read {
        Ok(read) => read,
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "chat.not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };

    let found_indices = read
        .messages
        .iter()
        .map(|message| message.index)
        .collect::<HashSet<_>>();
    let missing = requests
        .iter()
        .filter(|request| !found_indices.contains(&request.index))
        .map(|request| request.index)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Ok((
            tool_error(
                call,
                "chat.message_not_found",
                &format!(
                    "message index {} does not exist in this chat; total messages: {}",
                    missing[0], read.total_messages
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let by_index = read
        .messages
        .into_iter()
        .map(|message| (message.index, message))
        .collect::<HashMap<_, _>>();
    let mut rendered = Vec::with_capacity(requests.len());
    let mut total_returned_chars = 0_usize;
    for request in &requests {
        let message = by_index
            .get(&request.index)
            .expect("missing messages were checked above");
        let item = match render_message(message, request) {
            Ok(item) => item,
            Err(message) => {
                return Ok((
                    tool_error(call, "chat.invalid_message_range", &message),
                    AgentToolEffect::None,
                ));
            }
        };
        total_returned_chars += item.text.chars().count();
        if total_returned_chars > MAX_TOTAL_READ_CHARS {
            return Ok((
                tool_error(
                    call,
                    "chat.read_too_large",
                    &format!(
                        "read result exceeds {MAX_TOTAL_READ_CHARS} characters; read fewer messages or smaller ranges"
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        rendered.push(item);
    }

    let resource_refs = rendered
        .iter()
        .map(|message| message.ref_id.clone())
        .collect::<Vec<_>>();
    let content = render_content(read.total_messages, &rendered);

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "totalMessages": read.total_messages,
                "messages": rendered.iter().map(structured_message).collect::<Vec<_>>(),
            }),
            is_error: false,
            error_code: None,
            resource_refs,
        },
        AgentToolEffect::None,
    ))
}

fn parse_message_requests(args: &Map<String, Value>) -> Result<Vec<MessageRequest>, String> {
    let values = args
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "messages is required and must be an array".to_string())?;
    if values.is_empty() {
        return Err("messages must include at least one item".to_string());
    }
    if values.len() > MAX_MESSAGES_PER_READ {
        return Err(format!(
            "messages can include at most {MAX_MESSAGES_PER_READ} items"
        ));
    }

    values
        .iter()
        .enumerate()
        .map(|(position, value)| parse_message_request(position, value))
        .collect()
}

fn parse_message_request(position: usize, value: &Value) -> Result<MessageRequest, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("messages[{position}] must be an object"))?;
    let index = object
        .get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("messages[{position}].index must be a non-negative integer"))?;
    let index =
        usize::try_from(index).map_err(|_| format!("messages[{position}].index is too large"))?;
    let start_char = optional_request_usize(object, "start_char", position)?;
    let max_chars = optional_request_usize(object, "max_chars", position)?;
    if max_chars == Some(0) {
        return Err(format!("messages[{position}].max_chars must be >= 1"));
    }
    if max_chars.is_some_and(|value| value > MAX_MESSAGE_RANGE_CHARS) {
        return Err(format!(
            "messages[{position}].max_chars must be <= {MAX_MESSAGE_RANGE_CHARS}"
        ));
    }

    Ok(MessageRequest {
        index,
        start_char,
        max_chars,
    })
}

fn optional_request_usize(
    object: &Map<String, Value>,
    key: &str,
    position: usize,
) -> Result<Option<usize>, String> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let Some(value) = value.as_u64() else {
        return Err(format!(
            "messages[{position}].{key} must be a non-negative integer"
        ));
    };
    usize::try_from(value)
        .map(Some)
        .map_err(|_| format!("messages[{position}].{key} is too large"))
}

fn render_message(
    message: &ChatMessageReadItem,
    request: &MessageRequest,
) -> Result<RenderedMessage, String> {
    let total_chars = message.text.chars().count();
    let start_char = request.start_char.unwrap_or(0);
    if total_chars > 0 && start_char >= total_chars {
        return Err(format!(
            "message {} has {total_chars} characters; start_char {start_char} is outside the message",
            message.index
        ));
    }
    if total_chars == 0 && start_char > 0 {
        return Err(format!(
            "message {} is empty; start_char must be 0",
            message.index
        ));
    }

    if request.max_chars.is_none() && total_chars > MAX_FULL_MESSAGE_CHARS {
        return Err(format!(
            "message {} has {total_chars} characters; set start_char and max_chars to read it in ranges",
            message.index
        ));
    }

    let requested = request
        .max_chars
        .unwrap_or_else(|| total_chars.saturating_sub(start_char));
    let end_char = start_char.saturating_add(requested).min(total_chars);
    let text = slice_chars(&message.text, start_char, end_char);
    let truncated = end_char < total_chars;

    Ok(RenderedMessage {
        index: message.index,
        role: role_as_str(message.role),
        name: message.name.clone(),
        send_date: message.send_date.clone(),
        start_char,
        end_char,
        total_chars,
        truncated,
        text,
        ref_id: format!(
            "chat:current#{}:chars={}..{}",
            message.index, start_char, end_char
        ),
    })
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end - start).collect()
}

fn render_content(total_messages: usize, messages: &[RenderedMessage]) -> String {
    let mut content = format!(
        "Read {} message{} from current chat ({} total messages).",
        messages.len(),
        if messages.len() == 1 { "" } else { "s" },
        total_messages
    );
    for message in messages {
        content.push_str(&format!(
            "\n\nmessage {} {}{} chars {}-{} of {} ref {}",
            message.index,
            message.role,
            message
                .name
                .as_ref()
                .map(|name| format!(" {name}"))
                .unwrap_or_default(),
            message.start_char,
            message.end_char,
            message.total_chars,
            message.ref_id
        ));
        if let Some(send_date) = &message.send_date {
            content.push_str(&format!(" send_date {send_date}"));
        }
        if message.truncated {
            content.push_str(" truncated");
        }
        content.push('\n');
        content.push_str(&message.text);
    }
    content
}

fn structured_message(message: &RenderedMessage) -> Value {
    json!({
        "index": message.index,
        "role": message.role,
        "name": message.name.as_deref(),
        "sendDate": message.send_date.as_deref(),
        "startChar": message.start_char,
        "endChar": message.end_char,
        "totalChars": message.total_chars,
        "truncated": message.truncated,
        "text": message.text.as_str(),
        "ref": message.ref_id.as_str(),
    })
}
