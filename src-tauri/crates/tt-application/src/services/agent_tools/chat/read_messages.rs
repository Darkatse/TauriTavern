use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    MAX_MESSAGE_READ_CHARS, MAX_MESSAGE_READ_LINES, MAX_MESSAGES_PER_READ, MAX_TOTAL_READ_CHARS,
    chat_unavailable_message, role_as_str, visible_total_messages,
};
use crate::errors::ApplicationError;
use crate::services::agent_tools::common::{ensure_only_args, object_args, tool_error};
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::{FrozenMacros, MAX_EXPANDED_TEXT_BYTES};
use tt_domain::models::agent::{AgentChatRef, AgentToolResult};
use tt_domain::models::tool::ToolInvocation;
use tt_domain::text_lines::TextLineSelection;
use tt_domain::text_metrics::TextMetrics;
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::chat_repository::{ChatMessageReadItem, ChatRepository};
use tt_ports::repositories::group_chat_repository::GroupChatRepository;

use super::super::structured::{TextLineRangePayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatReadMessagesStructured<'a> {
    total_messages: usize,
    messages: Vec<ChatReadMessageStructured<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatReadMessageStructured<'a> {
    index: usize,
    role: &'static str,
    name: Option<&'a str>,
    send_date: Option<&'a str>,
    #[serde(flatten)]
    range: TextLineRangePayload,
    text: &'a str,
    #[serde(rename = "ref")]
    ref_id: &'a str,
}

#[derive(Debug, Clone)]
struct MessageRequest {
    index: usize,
    start_line: Option<usize>,
    line_count: Option<usize>,
}

struct RenderedMessage {
    index: usize,
    role: &'static str,
    name: Option<String>,
    send_date: Option<String>,
    selection: TextLineSelection,
    metrics: TextMetrics,
    total_metrics: TextMetrics,
    ref_id: String,
}

pub(in crate::services::agent_tools) async fn read_messages(
    run_repository: &dyn AgentRunRepository,
    chat_repository: &dyn ChatRepository,
    group_chat_repository: &dyn GroupChatRepository,
    run_id: &str,
    call: &ToolInvocation,
    macros: &FrozenMacros,
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
    if let Err(message) = ensure_only_args(args, &["messages"]) {
        return Ok((
            tool_error(call, "tool.invalid_arguments", &message),
            AgentToolEffect::None,
        ));
    }
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
    let mut read = match read {
        Ok(read) => read,
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "chat.not_found", &chat_unavailable_message(&message)),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    let visible_total = visible_total_messages(&run, read.total_messages)?;

    if let Some(request) = requests
        .iter()
        .find(|request| request.index >= visible_total)
    {
        return Ok((
            tool_error(
                call,
                "chat.message_not_found",
                &format!(
                    "Message index {} is not available; the current chat has {} visible messages. Search the current chat again, then choose one of the returned indexes.",
                    request.index, visible_total
                ),
            ),
            AgentToolEffect::None,
        ));
    }

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
                    "Message index {} is not available; the current chat has {} visible messages. Search the current chat again, then choose one of the returned indexes.",
                    missing[0], visible_total
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    for message in &mut read.messages {
        if let Cow::Owned(text) = macros.render(&message.text, MAX_EXPANDED_TEXT_BYTES)? {
            message.text = text;
        }
    }
    let by_index = read
        .messages
        .into_iter()
        .map(|message| (message.index, message))
        .collect::<HashMap<_, _>>();
    let per_message_chars = MAX_MESSAGE_READ_CHARS.min(MAX_TOTAL_READ_CHARS / requests.len());
    let mut rendered = Vec::with_capacity(requests.len());
    for request in &requests {
        let message = by_index
            .get(&request.index)
            .expect("missing messages were checked above");
        let item = match render_message(message, request, per_message_chars) {
            Ok(item) => item,
            Err(message) => {
                return Ok((
                    tool_error(call, "chat.invalid_message_range", &message),
                    AgentToolEffect::None,
                ));
            }
        };
        rendered.push(item);
    }

    let resource_refs = rendered
        .iter()
        .map(|message| message.ref_id.clone())
        .collect::<Vec<_>>();
    let content = render_content(visible_total, &rendered);

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: structured_value(ChatReadMessagesStructured {
                total_messages: visible_total,
                messages: rendered.iter().map(structured_message).collect(),
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
    for key in object.keys() {
        if key != "index" && key != "start_line" && key != "line_count" {
            return Err(format!("messages[{position}].{key} is not supported"));
        }
    }
    let index = object
        .get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("messages[{position}].index must be a non-negative integer"))?;
    let index =
        usize::try_from(index).map_err(|_| format!("messages[{position}].index is too large"))?;

    Ok(MessageRequest {
        index,
        start_line: optional_request_usize(object, "start_line", position)?,
        line_count: optional_request_usize(object, "line_count", position)?,
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
    max_chars: usize,
) -> Result<RenderedMessage, String> {
    let selection = TextLineSelection::select(
        &message.text,
        request.start_line.unwrap_or(1),
        request.line_count,
        MAX_MESSAGE_READ_LINES,
        max_chars,
    )
    .map_err(|error| format!("message {}: {error}", message.index))?;
    let metrics = TextMetrics::from_text(&selection.content);
    let total_metrics = TextMetrics::from_text(&message.text);
    let ref_id = format!(
        "chat:current#{}:L{}-L{}",
        message.index, selection.start_line, selection.end_line
    );

    Ok(RenderedMessage {
        index: message.index,
        role: role_as_str(message.role),
        name: message.name.clone(),
        send_date: message.send_date.clone(),
        selection,
        metrics,
        total_metrics,
        ref_id,
    })
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
            "\n\nmessage {} {}{} lines {}-{} of {}, chars {} of {}, words {} of {}, ref {}{}",
            message.index,
            message.role,
            message
                .name
                .as_ref()
                .map(|name| format!(" {name}"))
                .unwrap_or_default(),
            message.selection.start_line,
            message.selection.end_line,
            message.selection.total_lines,
            message.metrics.chars,
            message.total_metrics.chars,
            message.metrics.words,
            message.total_metrics.words,
            message.ref_id,
            if message.selection.truncated() {
                " (preview)"
            } else {
                ""
            },
        ));
        if let Some(send_date) = &message.send_date {
            content.push_str(&format!(" send_date {send_date}"));
        }
        let numbered = message.selection.numbered_content();
        if !numbered.is_empty() {
            content.push('\n');
            content.push_str(&numbered);
        }
        if let Some(next_start_line) = message.selection.next_start_line() {
            content.push_str(&format!(
                "\nContinue message {} with start_line={next_start_line} and line_count={}.",
                message.index,
                message.selection.returned_line_count()
            ));
        }
        if message.selection.line_truncated {
            content.push_str(&format!(
                "\nLine {} exceeds the read preview budget and was truncated.",
                message.selection.start_line
            ));
        }
    }
    content
}

fn structured_message(message: &RenderedMessage) -> ChatReadMessageStructured<'_> {
    ChatReadMessageStructured {
        index: message.index,
        role: message.role,
        name: message.name.as_deref(),
        send_date: message.send_date.as_deref(),
        range: TextLineRangePayload::new(
            message.metrics,
            message.total_metrics,
            message.selection.total_lines,
            message.selection.start_line,
            message.selection.end_line,
            message.selection.line_truncated,
        ),
        text: message.selection.content.as_str(),
        ref_id: message.ref_id.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tt_ports::repositories::chat_repository::{ChatMessageReadItem, ChatMessageRole};

    use super::{MAX_MESSAGE_READ_CHARS, MessageRequest, parse_message_request, render_message};

    #[test]
    fn long_messages_default_to_a_line_preview() {
        let message = ChatMessageReadItem {
            index: 7,
            role: ChatMessageRole::Assistant,
            name: None,
            send_date: None,
            text: format!("{}\n{}", "a".repeat(5_000), "b".repeat(5_000)),
        };
        let rendered = render_message(
            &message,
            &MessageRequest {
                index: 7,
                start_line: None,
                line_count: None,
            },
            MAX_MESSAGE_READ_CHARS,
        )
        .unwrap();

        assert_eq!(rendered.selection.start_line, 1);
        assert_eq!(rendered.selection.end_line, 1);
        assert_eq!(rendered.selection.next_start_line(), Some(2));
        assert!(rendered.selection.truncated());
    }

    #[test]
    fn character_ranges_are_not_accepted() {
        let error = parse_message_request(0, &json!({ "index": 7, "start_char": 0 })).unwrap_err();
        assert!(error.contains("start_char is not supported"));
    }
}
