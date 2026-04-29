use serde_json::{Map, Value, json};

use super::{
    DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT, MAX_SEARCH_SCAN_LIMIT, parse_role, role_as_str,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::common::{
    object_args, optional_usize_arg, required_trimmed_string_arg, tool_error,
};
use crate::application::services::agent_tools::dispatcher::AgentToolEffect;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentChatRef, AgentToolCall, AgentToolResult};
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::chat_repository::{
    ChatMessageSearchFilters, ChatMessageSearchHit, ChatMessageSearchQuery, ChatRepository,
};
use crate::domain::repositories::group_chat_repository::GroupChatRepository;

pub(in crate::application::services::agent_tools) async fn search(
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
    let query = match required_trimmed_string_arg(args, "query") {
        Some(query) => query.to_string(),
        None => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", "query is required"),
                AgentToolEffect::None,
            ));
        }
    };
    let search_query = match parse_search_query(args, query) {
        Ok(query) => query,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    let run = run_repository.load_run(run_id).await?;
    let hits = match &run.chat_ref {
        AgentChatRef::Character {
            character_id,
            file_name,
        } => {
            chat_repository
                .search_character_chat_messages(character_id, file_name, search_query.clone())
                .await
        }
        AgentChatRef::Group { chat_id } => {
            group_chat_repository
                .search_group_chat_messages(chat_id, search_query.clone())
                .await
        }
    };
    let hits = match hits {
        Ok(hits) => hits,
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "chat.not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };

    let content = render_content(&search_query.query, &hits);
    let resource_refs = hits
        .iter()
        .map(|hit| format!("chat:current#{}", hit.index))
        .collect::<Vec<_>>();

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "query": search_query.query,
                "hits": hits.iter().map(structured_hit).collect::<Vec<_>>(),
            }),
            is_error: false,
            error_code: None,
            resource_refs,
        },
        AgentToolEffect::None,
    ))
}

fn parse_search_query(
    args: &Map<String, Value>,
    query: String,
) -> Result<ChatMessageSearchQuery, String> {
    let limit = optional_usize_arg(args, "limit")?.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if limit == 0 {
        return Err("limit must be >= 1".to_string());
    }
    if limit > MAX_SEARCH_LIMIT {
        return Err(format!("limit must be <= {MAX_SEARCH_LIMIT}"));
    }

    let role = match args.get("role") {
        Some(Value::String(value)) => Some(
            parse_role(value)
                .ok_or_else(|| "role must be user, assistant, or system".to_string())?,
        ),
        Some(_) => return Err("role must be a string".to_string()),
        None => None,
    };
    let start_index = optional_usize_arg(args, "start_message")?;
    let end_index = optional_usize_arg(args, "end_message")?;
    if matches!((start_index, end_index), (Some(start), Some(end)) if start > end) {
        return Err("start_message must be <= end_message".to_string());
    }
    let scan_limit = optional_usize_arg(args, "scan_limit")?;
    if scan_limit == Some(0) {
        return Err("scan_limit must be >= 1".to_string());
    }
    if scan_limit.is_some_and(|value| value > MAX_SEARCH_SCAN_LIMIT) {
        return Err(format!("scan_limit must be <= {MAX_SEARCH_SCAN_LIMIT}"));
    }

    let filters =
        if role.is_some() || start_index.is_some() || end_index.is_some() || scan_limit.is_some() {
            Some(ChatMessageSearchFilters {
                role,
                start_index,
                end_index,
                scan_limit,
            })
        } else {
            None
        };

    Ok(ChatMessageSearchQuery {
        query,
        limit,
        filters,
    })
}

fn render_content(query: &str, hits: &[ChatMessageSearchHit]) -> String {
    if hits.is_empty() {
        return format!("No messages matched `{query}` in the current chat.");
    }

    let mut content = format!(
        "Search `{query}` matched {} message{} in the current chat. Use chat_read_messages with the message index to read exact text.",
        hits.len(),
        if hits.len() == 1 { "" } else { "s" }
    );
    for hit in hits {
        content.push_str(&format!(
            "\n\nmessage {} {} score {:.3} ref chat:current#{}\n{}",
            hit.index,
            role_as_str(hit.role),
            hit.score,
            hit.index,
            hit.snippet
        ));
    }
    content
}

fn structured_hit(hit: &ChatMessageSearchHit) -> Value {
    json!({
        "index": hit.index,
        "role": role_as_str(hit.role),
        "score": hit.score,
        "snippet": hit.snippet,
        "ref": format!("chat:current#{}", hit.index),
    })
}
