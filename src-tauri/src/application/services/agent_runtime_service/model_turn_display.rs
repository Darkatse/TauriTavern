use serde::Deserialize;
use serde_json::{Value, json};

use super::AgentRuntimeService;
use crate::application::dto::agent_dto::{
    AgentModelTurnDisplayDto, AgentModelTurnProviderDto, AgentModelTurnReasoningDto,
    AgentModelTurnTextDto, AgentModelTurnToolCallDto, AgentReadModelTurnDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelResponse, ROOT_AGENT_INVOCATION_ID, WorkspacePath,
};
use crate::domain::text_metrics::TextMetrics;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredModelResponse {
    round: usize,
    #[serde(default)]
    invocation_id: Option<String>,
    response: AgentModelResponse,
}

impl AgentRuntimeService {
    pub async fn read_model_turn(
        &self,
        dto: AgentReadModelTurnDto,
    ) -> Result<AgentModelTurnDisplayDto, ApplicationError> {
        if dto.round == 0 {
            return Err(ApplicationError::ValidationError(
                "agent.model_turn_round_invalid: round must be greater than zero".to_string(),
            ));
        }
        if dto.max_chars == 0 {
            return Err(ApplicationError::ValidationError(
                "agent.model_turn_max_chars_invalid: maxChars must be greater than zero"
                    .to_string(),
            ));
        }

        let invocation_id = normalize_model_turn_invocation_id(dto.invocation_id.as_deref())?;
        let path = model_response_path_for_invocation(&invocation_id, dto.round)?;
        let file = self
            .workspace_repository
            .read_text(&dto.run_id, &path)
            .await?;
        let document: StoredModelResponse = serde_json::from_str(&file.text).map_err(|error| {
            ApplicationError::ValidationError(format!("agent.model_response_parse_failed: {error}"))
        })?;
        if document.round != dto.round {
            return Err(ApplicationError::ValidationError(format!(
                "agent.model_response_round_mismatch: requested round {}, stored round {}",
                dto.round, document.round
            )));
        }
        if let Some(stored_invocation_id) = document.invocation_id.as_deref() {
            if stored_invocation_id != invocation_id {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.model_response_invocation_mismatch: requested invocation {}, stored invocation {}",
                    invocation_id, stored_invocation_id
                )));
            }
        }

        Ok(project_model_turn(
            &dto.run_id,
            path.as_str(),
            document.round,
            &document.response,
            dto.max_chars,
        ))
    }
}

pub(super) fn model_response_path_for_invocation(
    invocation_id: &str,
    round: usize,
) -> Result<WorkspacePath, ApplicationError> {
    if invocation_id == ROOT_AGENT_INVOCATION_ID {
        return Ok(WorkspacePath::parse(format!(
            "model-responses/round-{round:03}.json"
        ))?);
    }
    Ok(WorkspacePath::parse(format!(
        "model-responses/{invocation_id}/round-{round:03}.json"
    ))?)
}

fn normalize_model_turn_invocation_id(value: Option<&str>) -> Result<String, ApplicationError> {
    let invocation_id = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(ROOT_AGENT_INVOCATION_ID);
    if invocation_id.contains('/') || invocation_id.contains('\\') {
        return Err(ApplicationError::ValidationError(
            "agent.model_turn_invocation_id_invalid: invocationId must not contain path separators"
                .to_string(),
        ));
    }
    Ok(invocation_id.to_string())
}

pub(super) fn model_turn_event_summary(response: &AgentModelResponse) -> Value {
    let assistant_metrics = TextMetrics::from_text(&response.text);
    let reasoning_metrics = response
        .message
        .parts
        .iter()
        .filter_map(reasoning_text)
        .map(TextMetrics::from_text)
        .fold(TextMetrics { chars: 0, words: 0 }, |total, metrics| {
            TextMetrics {
                chars: total.chars + metrics.chars,
                words: total.words + metrics.words,
            }
        });

    json!({
        "hasAssistantText": !response.text.trim().is_empty(),
        "assistantTextChars": assistant_metrics.chars,
        "assistantTextWords": assistant_metrics.words,
        "hasReasoning": reasoning_metrics.chars > 0,
        "reasoningChars": reasoning_metrics.chars,
        "reasoningWords": reasoning_metrics.words,
    })
}

fn project_model_turn(
    run_id: &str,
    model_response_path: &str,
    round: usize,
    response: &AgentModelResponse,
    max_chars: usize,
) -> AgentModelTurnDisplayDto {
    AgentModelTurnDisplayDto {
        run_id: run_id.to_string(),
        round,
        model_response_path: model_response_path.to_string(),
        provider: project_provider(response),
        assistant: text_dto(&response.text, max_chars),
        reasoning: response
            .message
            .parts
            .iter()
            .filter_map(|part| reasoning_dto(part, max_chars))
            .collect(),
        tool_calls: response
            .tool_calls
            .iter()
            .map(|call| AgentModelTurnToolCallDto {
                call_id: call.id.clone(),
                name: call.name.clone(),
                model_name: call
                    .provider_metadata
                    .get("modelName")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
            .collect(),
    }
}

fn project_provider(response: &AgentModelResponse) -> AgentModelTurnProviderDto {
    AgentModelTurnProviderDto {
        source: string_field(&response.provider_metadata, "chatCompletionSource"),
        format: string_field(&response.provider_metadata, "providerFormat"),
        model: string_field(&response.provider_metadata, "model"),
        response_id: string_field(&response.provider_metadata, "id"),
        usage: response
            .provider_metadata
            .get("usage")
            .cloned()
            .unwrap_or(Value::Null),
    }
}

fn reasoning_dto(
    part: &AgentModelContentPart,
    max_chars: usize,
) -> Option<AgentModelTurnReasoningDto> {
    let AgentModelContentPart::Reasoning {
        text,
        provider_metadata,
    } = part
    else {
        return None;
    };
    let text = text.as_deref()?.trim();
    if text.is_empty() {
        return None;
    }

    let display = text_dto(text, max_chars);
    Some(AgentModelTurnReasoningDto {
        source: string_field(provider_metadata, "source")
            .unwrap_or_else(|| "reasoning_content".to_string()),
        text: display.text,
        total_chars: display.total_chars,
        total_words: display.total_words,
        truncated: display.truncated,
    })
}

fn reasoning_text(part: &AgentModelContentPart) -> Option<&str> {
    let AgentModelContentPart::Reasoning { text, .. } = part else {
        return None;
    };
    text.as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn text_dto(text: &str, max_chars: usize) -> AgentModelTurnTextDto {
    let metrics = TextMetrics::from_text(text);
    let (text, truncated) = truncate_chars(text, max_chars);
    AgentModelTurnTextDto {
        text,
        total_chars: metrics.chars,
        total_words: metrics.words,
        truncated,
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    for (count, (index, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            return (text[..index].to_string(), true);
        }
    }
    (text.to_string(), false)
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
