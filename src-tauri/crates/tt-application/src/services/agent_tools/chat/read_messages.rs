use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    MAX_MESSAGE_READ_CHARS, MAX_MESSAGE_READ_LINES, MAX_MESSAGES_PER_READ, MAX_TOTAL_READ_CHARS,
    chat_read_error,
};
use crate::errors::ApplicationError;
use crate::services::agent_tools::common::tool_error;
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use crate::services::agent_workspace_scope::{ChatFloor, ChatSnapshot, FloorRole};
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;
use tt_domain::text_lines::TextLineSelection;
use tt_domain::text_metrics::TextMetrics;

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
    #[serde(flatten)]
    role: FloorRole,
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
    floor: usize,
    offset: Option<usize>,
    limit: Option<usize>,
}

/// The requested lines of floor `index`; its role, name and date are read from the floor.
struct RenderedMessage {
    index: usize,
    selection: TextLineSelection,
    metrics: TextMetrics,
    total_metrics: TextMetrics,
    ref_id: String,
}

pub(in crate::services::agent_tools) async fn read_messages(
    chat: &ChatSnapshot,
    call: &ToolInvocation,
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let requests = match parse_message_requests(args) {
        Ok(requests) => requests,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    let floors = match chat.floors().await {
        Ok(floors) => floors,
        Err(error) => return chat_read_error(call, error),
    };
    let visible_total = floors.len();
    if let Some(request) = requests
        .iter()
        .find(|request| request.floor >= visible_total)
    {
        return Ok((
            tool_error(
                call,
                "chat.message_not_found",
                &format!(
                    "Floor {} is not available; the current chat has {} floors. Search the current chat again, then choose one of the returned floor numbers.",
                    request.floor, visible_total
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let per_message_chars = MAX_MESSAGE_READ_CHARS.min(MAX_TOTAL_READ_CHARS / requests.len());
    let mut rendered = Vec::with_capacity(requests.len());
    for request in &requests {
        let floor = &floors[request.floor];
        let Some(text) = floor.message.as_deref() else {
            return Ok((
                tool_error(
                    call,
                    "chat.message_not_found",
                    &format!(
                        "Floor {} has no string `mes` field, so it has no text.",
                        request.floor
                    ),
                ),
                AgentToolEffect::None,
            ));
        };
        let item = match render_message(text, request, per_message_chars) {
            Ok(item) => (floor, item),
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
        .map(|(_, message)| message.ref_id.clone())
        .collect::<Vec<_>>();
    let content = render_content(visible_total, &rendered);

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: structured_value(ChatReadMessagesStructured {
                total_messages: visible_total,
                messages: rendered
                    .iter()
                    .map(|(floor, message)| structured_message(floor, message))
                    .collect(),
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
        .get("floors")
        .and_then(Value::as_array)
        .ok_or_else(|| "floors is required and must be an array".to_string())?;
    if values.is_empty() {
        return Err("floors must include at least one item".to_string());
    }
    if values.len() > MAX_MESSAGES_PER_READ {
        return Err(format!(
            "floors can include at most {MAX_MESSAGES_PER_READ} items"
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
        .ok_or_else(|| format!("floors[{position}] must be an object"))?;
    let floor = object
        .get("floor")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("floors[{position}].floor must be a non-negative integer"))?;
    let floor =
        usize::try_from(floor).map_err(|_| format!("floors[{position}].floor is too large"))?;

    Ok(MessageRequest {
        floor,
        offset: optional_request_usize(object, "offset", position)?,
        limit: optional_request_usize(object, "limit", position)?,
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
            "floors[{position}].{key} must be a non-negative integer"
        ));
    };
    usize::try_from(value)
        .map(Some)
        .map_err(|_| format!("floors[{position}].{key} is too large"))
}

fn render_message(
    text: &str,
    request: &MessageRequest,
    max_chars: usize,
) -> Result<RenderedMessage, String> {
    let index = request.floor;
    let selection = TextLineSelection::select(
        text,
        request.offset.unwrap_or(1),
        request.limit,
        MAX_MESSAGE_READ_LINES,
        max_chars,
    )
    .map_err(|error| format!("floor {index}: {error}"))?;
    let metrics = TextMetrics::from_text(&selection.content);
    let total_metrics = TextMetrics::from_text(text);
    let ref_id = format!(
        "chat:current#{index}:L{}-L{}",
        selection.start_line, selection.end_line
    );

    Ok(RenderedMessage {
        index,
        selection,
        metrics,
        total_metrics,
        ref_id,
    })
}

fn render_content(total_messages: usize, messages: &[(&ChatFloor, RenderedMessage)]) -> String {
    let mut content = format!(
        "Read {} floor{} from the current chat ({} floors in total).",
        messages.len(),
        if messages.len() == 1 { "" } else { "s" },
        total_messages
    );
    for (floor, message) in messages {
        content.push_str(&format!(
            "\n\nfloor {} {}{} lines {}-{} of {}, chars {} of {}, words {} of {}, ref {}{}",
            message.index,
            floor.role,
            floor
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
        if let Some(send_date) = &floor.send_date {
            content.push_str(&format!(" send_date {send_date}"));
        }
        let numbered = message.selection.numbered_content();
        if !numbered.is_empty() {
            content.push('\n');
            content.push_str(&numbered);
        }
        if let Some(next_start_line) = message.selection.next_start_line() {
            content.push_str(&format!(
                "\nContinue floor {} with offset={next_start_line} and limit={}.",
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

fn structured_message<'a>(
    floor: &'a ChatFloor,
    message: &'a RenderedMessage,
) -> ChatReadMessageStructured<'a> {
    ChatReadMessageStructured {
        index: message.index,
        role: floor.role,
        name: floor.name.as_deref(),
        send_date: floor.send_date.as_deref(),
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
    use super::{MAX_MESSAGE_READ_CHARS, MessageRequest, render_message};

    #[test]
    fn long_messages_default_to_a_line_preview() {
        let text = format!("{}\n{}", "a".repeat(5_000), "b".repeat(5_000));
        let rendered = render_message(
            &text,
            &MessageRequest {
                floor: 7,
                offset: None,
                limit: None,
            },
            MAX_MESSAGE_READ_CHARS,
        )
        .unwrap();

        assert_eq!(rendered.selection.start_line, 1);
        assert_eq!(rendered.selection.end_line, 1);
        assert_eq!(rendered.selection.next_start_line(), Some(2));
        assert!(rendered.selection.truncated());
    }
}
