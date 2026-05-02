use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::sync::watch;

use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::application::services::chat_completion_service::exchange::{
    ChatCompletionExchange, ChatCompletionProviderFormat, NormalizedChatCompletionResponse,
};
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelResponse,
    AgentModelRole, AgentToolCall, AgentToolSpec,
};
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

#[async_trait]
pub trait AgentModelGateway: Send + Sync {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelResponse, ApplicationError>;
}

pub struct ChatCompletionAgentModelGateway {
    chat_completion_service: Arc<ChatCompletionService>,
}

impl ChatCompletionAgentModelGateway {
    pub fn new(chat_completion_service: Arc<ChatCompletionService>) -> Self {
        Self {
            chat_completion_service,
        }
    }
}

#[async_trait]
impl AgentModelGateway for ChatCompletionAgentModelGateway {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelResponse, ApplicationError> {
        let dto = encode_chat_completion_request(&request)?;
        let exchange = self
            .chat_completion_service
            .generate_exchange_with_cancel(dto, cancel)
            .await?;
        decode_chat_completion_exchange(exchange, &request.tools)
    }
}

pub(crate) fn encode_chat_completion_request(
    request: &AgentModelRequest,
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let source = ChatCompletionSource::parse(
        request
            .payload
            .get("chat_completion_source")
            .and_then(Value::as_str)
            .unwrap_or("openai"),
    )
    .ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.model_request_invalid_source: unsupported chat completion source".to_string(),
        )
    })?;
    let provider_format = ChatCompletionProviderFormat::from_payload(source, &request.payload)?;
    let mut payload = request.payload.clone();

    payload.insert(
        "messages".to_string(),
        Value::Array(
            request
                .messages
                .iter()
                .map(|message| encode_openai_compatible_message(message, &request.tools))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );

    if !request.tools.is_empty() {
        payload.insert(
            "tools".to_string(),
            Value::Array(render_openai_tools(&request.tools, provider_format)),
        );
        payload.insert(
            "tool_choice".to_string(),
            if request.tool_choice.is_null() {
                Value::String("auto".to_string())
            } else {
                request.tool_choice.clone()
            },
        );
    }

    if provider_format == ChatCompletionProviderFormat::OpenAiResponses {
        ensure_openai_responses_reasoning_include(&mut payload);
    }

    payload.insert("stream".to_string(), Value::Bool(false));
    Ok(ChatCompletionGenerateRequestDto { payload })
}

#[cfg(test)]
pub(crate) fn decode_chat_completion_response(
    response: Value,
    tools: &[AgentToolSpec],
) -> Result<AgentModelResponse, ApplicationError> {
    let normalized = NormalizedChatCompletionResponse::from_value(response)?;
    decode_normalized_chat_completion_response(&normalized, tools)
}

fn decode_chat_completion_exchange(
    exchange: ChatCompletionExchange,
    tools: &[AgentToolSpec],
) -> Result<AgentModelResponse, ApplicationError> {
    if !exchange
        .normalization_report
        .synthetic_tool_call_ids()
        .is_empty()
    {
        return Err(ApplicationError::ValidationError(format!(
            "model.invalid_tool_call: provider response is missing tool_call_id for tool calls: {}",
            exchange
                .normalization_report
                .synthetic_tool_call_ids()
                .join(", ")
        )));
    }

    let mut response =
        decode_normalized_chat_completion_response(&exchange.normalized_response, tools)?;
    let provider_metadata = response.provider_metadata.clone();
    response.provider_metadata = json!({
        "id": provider_metadata.get("id"),
        "model": provider_metadata.get("model"),
        "usage": provider_metadata.get("usage"),
        "chatCompletionSource": exchange.source.key(),
        "providerFormat": exchange.provider_format.key(),
    });
    Ok(response)
}

fn decode_normalized_chat_completion_response(
    response: &NormalizedChatCompletionResponse,
    tools: &[AgentToolSpec],
) -> Result<AgentModelResponse, ApplicationError> {
    let message = response.assistant_message();
    let raw_response = response.raw();

    let text = extract_text_from_message(message);
    let tool_calls = extract_tool_calls_from_message(message, tools)?;
    let mut parts = Vec::new();

    if !text.trim().is_empty() {
        parts.push(AgentModelContentPart::Text { text: text.clone() });
    }

    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(AgentModelContentPart::Reasoning {
            text: Some(reasoning),
            provider_metadata: json!({ "source": "reasoning_content" }),
        });
    }

    if let Some(native) = message.get("native").and_then(Value::as_object) {
        for (provider, value) in native {
            parts.push(AgentModelContentPart::Native {
                provider: provider.clone(),
                value: value.clone(),
            });
        }
    }

    for call in &tool_calls {
        parts.push(AgentModelContentPart::ToolCall { call: call.clone() });
    }

    let model_message = AgentModelMessage {
        role: AgentModelRole::Assistant,
        parts,
        provider_metadata: json!({
            "message": message,
            "responseId": raw_response.get("id"),
            "model": raw_response.get("model"),
        }),
    };

    Ok(AgentModelResponse {
        message: model_message,
        tool_calls,
        text,
        provider_metadata: json!({
            "id": raw_response.get("id"),
            "model": raw_response.get("model"),
            "usage": raw_response.get("usage"),
        }),
        raw_response: raw_response.clone(),
    })
}

fn encode_openai_compatible_message(
    message: &AgentModelMessage,
    tools: &[AgentToolSpec],
) -> Result<Value, ApplicationError> {
    let mut object = Map::new();
    object.insert(
        "role".to_string(),
        Value::String(
            match message.role {
                AgentModelRole::System => "system",
                AgentModelRole::Developer => "developer",
                AgentModelRole::User => "user",
                AgentModelRole::Assistant => "assistant",
                AgentModelRole::Tool => "tool",
            }
            .to_string(),
        ),
    );
    if let Some(name) = message
        .provider_metadata
        .pointer("/openai/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("name".to_string(), Value::String(name.to_string()));
    }

    match message.role {
        AgentModelRole::Assistant => {
            let content = openai_content_from_parts(&message.parts);
            object.insert("content".to_string(), content);

            let tool_calls = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    AgentModelContentPart::ToolCall { call } => Some(call),
                    _ => None,
                })
                .map(|call| encode_openai_tool_call(call, tools))
                .collect::<Result<Vec<_>, _>>()?;
            if !tool_calls.is_empty() {
                object.insert("tool_calls".to_string(), Value::Array(tool_calls));
            }
        }
        AgentModelRole::Tool => {
            let result = message
                .parts
                .iter()
                .find_map(|part| match part {
                    AgentModelContentPart::ToolResult { result } => Some(result),
                    _ => None,
                })
                .ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "agent.invalid_model_message: tool message is missing tool result"
                            .to_string(),
                    )
                })?;

            object.insert(
                "tool_call_id".to_string(),
                Value::String(result.call_id.clone()),
            );
            object.insert("name".to_string(), Value::String(result.name.clone()));
            object.insert(
                "content".to_string(),
                Value::String(tool_result_message_content(result)?),
            );
        }
        _ => {
            object.insert(
                "content".to_string(),
                openai_content_from_parts(&message.parts),
            );
        }
    }

    let mut native = Map::new();
    let mut reasoning = Vec::new();
    for part in &message.parts {
        match part {
            AgentModelContentPart::Native { provider, value } => {
                native.insert(provider.clone(), value.clone());
            }
            AgentModelContentPart::Reasoning { text, .. } => {
                if let Some(text) = text.as_ref().filter(|value| !value.trim().is_empty()) {
                    reasoning.push(text.clone());
                }
            }
            _ => {}
        }
    }

    if !native.is_empty() {
        object.insert("native".to_string(), Value::Object(native));
    }
    if !reasoning.is_empty() {
        object.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning.join("\n\n")),
        );
    }

    Ok(Value::Object(object))
}

fn openai_content_from_parts(parts: &[AgentModelContentPart]) -> Value {
    let mut text = String::new();
    let mut content_parts = Vec::new();
    let mut needs_array = false;

    for part in parts {
        match part {
            AgentModelContentPart::Text { text: part_text } => {
                if needs_array {
                    content_parts.push(json!({ "type": "text", "text": part_text }));
                } else {
                    text.push_str(part_text);
                }
            }
            AgentModelContentPart::Media { value, .. } => {
                if !text.is_empty() {
                    content_parts.push(json!({ "type": "text", "text": text }));
                    text = String::new();
                }
                needs_array = true;
                content_parts.push(value.clone());
            }
            AgentModelContentPart::Native { provider, value }
                if provider == "openai.content_part" =>
            {
                if !text.is_empty() {
                    content_parts.push(json!({ "type": "text", "text": text }));
                    text = String::new();
                }
                needs_array = true;
                content_parts.push(value.clone());
            }
            _ => {}
        }
    }

    if needs_array {
        if !text.is_empty() {
            content_parts.push(json!({ "type": "text", "text": text }));
        }
        Value::Array(content_parts)
    } else if text.is_empty() {
        Value::Null
    } else {
        Value::String(text)
    }
}

fn render_openai_tools(
    tools: &[AgentToolSpec],
    provider_format: ChatCompletionProviderFormat,
) -> Vec<Value> {
    tools
        .iter()
        .map(|spec| {
            json!({
                "type": "function",
                "function": {
                    "name": spec.model_name.as_str(),
                    "description": spec.description.as_str(),
                    "parameters": sanitize_schema_for_provider(&spec.input_schema, provider_format),
                }
            })
        })
        .collect()
}

fn sanitize_schema_for_provider(
    schema: &Value,
    provider_format: ChatCompletionProviderFormat,
) -> Value {
    let mut schema = schema.clone();
    let keys = match provider_format {
        ChatCompletionProviderFormat::Gemini | ChatCompletionProviderFormat::GeminiInteractions => {
            &[
                "$schema",
                "$id",
                "$defs",
                "definitions",
                "additionalProperties",
                "patternProperties",
                "unevaluatedProperties",
                "dependencies",
                "dependentRequired",
                "dependentSchemas",
                "allOf",
                "anyOf",
                "oneOf",
                "not",
                "if",
                "then",
                "else",
                "const",
                "default",
                "examples",
                "title",
            ][..]
        }
        ChatCompletionProviderFormat::ClaudeMessages => &["$schema", "$id"][..],
        ChatCompletionProviderFormat::OpenAiCompatible
        | ChatCompletionProviderFormat::OpenAiResponses => &[][..],
    };
    remove_schema_keys(&mut schema, keys);
    schema
}

fn remove_schema_keys(value: &mut Value, keys: &[&str]) {
    match value {
        Value::Object(object) => {
            for key in keys {
                object.remove(*key);
            }
            for nested in object.values_mut() {
                remove_schema_keys(nested, keys);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_schema_keys(item, keys);
            }
        }
        _ => {}
    }
}

fn ensure_openai_responses_reasoning_include(payload: &mut Map<String, Value>) {
    let entry = payload
        .entry("include".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(items) = entry.as_array_mut() else {
        return;
    };
    let encrypted = Value::String("reasoning.encrypted_content".to_string());
    if !items.iter().any(|item| item == &encrypted) {
        items.push(encrypted);
    }
}

fn encode_openai_tool_call(
    call: &AgentToolCall,
    tools: &[AgentToolSpec],
) -> Result<Value, ApplicationError> {
    let model_name = model_tool_name_for_call(&call.name, tools);
    let arguments = serde_json::to_string(&call.arguments).map_err(|error| {
        ApplicationError::ValidationError(format!("agent.tool_call_serialize_failed: {error}"))
    })?;

    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(call.id.clone()));
    object.insert("type".to_string(), Value::String("function".to_string()));
    object.insert(
        "function".to_string(),
        json!({
            "name": model_name,
            "arguments": arguments,
        }),
    );

    if let Some(signature) = call
        .provider_metadata
        .get("signature")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        object.insert("signature".to_string(), Value::String(signature));
    }

    Ok(Value::Object(object))
}

fn model_tool_name_for_call(name: &str, tools: &[AgentToolSpec]) -> String {
    tools
        .iter()
        .find(|spec| spec.name == name || spec.model_name == name)
        .map(|spec| spec.model_name.clone())
        .unwrap_or_else(|| name.to_string())
}

fn extract_tool_calls_from_message(
    message: &Map<String, Value>,
    tools: &[AgentToolSpec],
) -> Result<Vec<AgentToolCall>, ApplicationError> {
    let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    calls
        .iter()
        .map(|call| parse_tool_call(call, tools))
        .collect()
}

fn parse_tool_call(
    call: &Value,
    tools: &[AgentToolSpec],
) -> Result<AgentToolCall, ApplicationError> {
    let object = call.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "model.invalid_tool_call: tool call must be an object".to_string(),
        )
    })?;
    let function = object
        .get("function")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_call: tool call is missing function".to_string(),
            )
        })?;
    let raw_name = function
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_call: tool call function name is required".to_string(),
            )
        })?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_call: tool_call_id is required".to_string(),
            )
        })?;
    let canonical_name = canonical_tool_name(raw_name, tools).unwrap_or(raw_name);
    let arguments =
        parse_tool_call_arguments(function.get("arguments").or_else(|| function.get("args")));

    Ok(AgentToolCall {
        id: id.to_string(),
        name: canonical_name.to_string(),
        arguments,
        provider_metadata: json!({
            "modelName": raw_name,
            "signature": object.get("signature"),
            "raw": call,
        }),
    })
}

fn canonical_tool_name<'a>(raw: &'a str, tools: &'a [AgentToolSpec]) -> Option<&'a str> {
    tools
        .iter()
        .find(|spec| spec.model_name == raw || spec.name == raw)
        .map(|spec| spec.name.as_str())
}

fn parse_tool_call_arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(raw)) => {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
        }
        Some(Value::Null) | None => Value::Object(Map::new()),
        Some(value) => value.clone(),
    }
}

fn extract_text_from_message(message: &Map<String, Value>) -> String {
    text_from_value(message.get("content")).unwrap_or_default()
}

fn text_from_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut output = String::new();
            for part in parts {
                match part {
                    Value::String(text) => output.push_str(text),
                    Value::Object(object) => {
                        if object.get("type").and_then(Value::as_str) == Some("tool_use") {
                            return None;
                        }
                        if let Some(text) = object.get("text").and_then(Value::as_str) {
                            output.push_str(text);
                        } else if let Some(text) = object.get("content").and_then(Value::as_str) {
                            output.push_str(text);
                        }
                    }
                    _ => {}
                }
            }
            Some(output)
        }
        _ => None,
    }
}

fn tool_result_message_content(
    result: &crate::domain::models::agent::AgentToolResult,
) -> Result<String, ApplicationError> {
    serde_json::to_string(&json!({
        "ok": !result.is_error,
        "content": result.content.as_str(),
        "structured": &result.structured,
        "errorCode": result.error_code.as_deref(),
        "resourceRefs": &result.resource_refs,
    }))
    .map_err(|error| {
        ApplicationError::ValidationError(format!("agent.tool_result_serialize_failed: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::agent_tools::BuiltinAgentToolRegistry;
    use crate::domain::repositories::chat_completion_repository::ChatCompletionNormalizationReport;

    #[test]
    fn decodes_tool_call_to_canonical_name() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let response = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"hello\"}"
                        },
                        "signature": "sig_1"
                    }]
                }
            }]
        });

        let decoded = decode_chat_completion_response(response, registry.specs()).unwrap();
        assert_eq!(decoded.tool_calls.len(), 1);
        assert_eq!(decoded.tool_calls[0].name, "workspace.write_file");
        assert_eq!(decoded.tool_calls[0].id, "call_1");
        assert_eq!(
            decoded.tool_calls[0].provider_metadata["signature"],
            "sig_1"
        );
    }

    #[test]
    fn rejects_tool_call_without_id() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "type": "function",
                        "function": { "name": "workspace_finish", "arguments": "{}" }
                    }]
                }
            }]
        });

        let error = decode_chat_completion_response(response, registry.specs()).unwrap_err();
        assert!(error.to_string().contains("tool_call_id is required"));
    }

    #[test]
    fn rejects_normalizer_synthetic_tool_call_id() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "tool_call_0",
                        "type": "function",
                        "function": { "name": "workspace_finish", "arguments": "{}" }
                    }]
                }
            }]
        });
        let mut report = ChatCompletionNormalizationReport::default();
        report.record_synthetic_tool_call_id("tool_call_0");
        let exchange = ChatCompletionExchange {
            source: ChatCompletionSource::Claude,
            provider_format: ChatCompletionProviderFormat::ClaudeMessages,
            normalized_response: NormalizedChatCompletionResponse::from_value(response).unwrap(),
            normalization_report: report,
        };

        let error = decode_chat_completion_exchange(exchange, registry.specs()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("provider response is missing tool_call_id")
        );
    }

    #[test]
    fn gemini_schema_sanitizer_removes_unsupported_keys() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "$defs": { "x": { "type": "string" } },
            "properties": {
                "mode": {
                    "type": "string",
                    "const": "draft",
                    "default": "draft"
                }
            }
        });

        let sanitized = sanitize_schema_for_provider(&schema, ChatCompletionProviderFormat::Gemini);
        assert!(sanitized.get("additionalProperties").is_none());
        assert!(sanitized.get("$defs").is_none());
        assert!(sanitized["properties"]["mode"].get("const").is_none());
        assert!(sanitized["properties"]["mode"].get("default").is_none());
    }
}
