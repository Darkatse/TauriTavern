use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::errors::ApplicationError;
use crate::services::hashing::hex_lower;
use tt_ports::repositories::chat_completion_repository::{
    AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionSource,
};
use tt_ports::repositories::prompt_cache_repository::PromptCacheKey;

use super::model_capabilities::is_openrouter_claude_model_name;

const CUSTOM_CLAUDE_PROMPT_CACHING_FIELD: &str = "custom_claude_prompt_caching";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PromptCachingRequestHints {
    pub custom_claude_prompt_caching: bool,
}

impl PromptCachingRequestHints {
    pub(super) fn from_payload(payload: &Map<String, Value>) -> Result<Self, ApplicationError> {
        let custom_claude_prompt_caching = payload
            .get(CUSTOM_CLAUDE_PROMPT_CACHING_FIELD)
            .map(|value| {
                value.as_bool().ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "Chat completion request field must be a boolean: {}",
                        CUSTOM_CLAUDE_PROMPT_CACHING_FIELD
                    ))
                })
            })
            .transpose()?
            .unwrap_or(false);

        Ok(Self {
            custom_claude_prompt_caching,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PromptCachingPlan {
    Claude {
        key: PromptCacheKey,
        anthropic_beta_header_mode: AnthropicBetaHeaderMode,
    },
    OpenRouterClaude {
        key: PromptCacheKey,
    },
    VertexClaude {
        key: PromptCacheKey,
        model: String,
    },
    NanoGptClaude,
}

pub(super) fn resolve_prompt_caching_plan(
    source: ChatCompletionSource,
    endpoint_path: &str,
    config: &ChatCompletionApiConfig,
    upstream_payload: &Value,
    hints: PromptCachingRequestHints,
) -> Result<Option<PromptCachingPlan>, ApplicationError> {
    match source {
        ChatCompletionSource::Claude => Ok(Some(PromptCachingPlan::Claude {
            key: PromptCacheKey::Claude,
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::ClaudeDefaults,
        })),
        ChatCompletionSource::OpenRouter => Ok(is_openrouter_claude_model(upstream_payload)
            .then_some(PromptCachingPlan::OpenRouterClaude {
                key: PromptCacheKey::OpenRouterClaude,
            })),
        ChatCompletionSource::VertexAi => {
            Ok(vertexai_claude_model_id(endpoint_path).map(|model| {
                PromptCachingPlan::VertexClaude {
                    key: PromptCacheKey::VertexAiClaude {
                        scope: vertexai_claude_prompt_cache_scope(config, endpoint_path),
                    },
                    model: model.to_string(),
                }
            }))
        }
        ChatCompletionSource::NanoGpt => {
            Ok(is_nanogpt_claude_payload(upstream_payload)
                .then_some(PromptCachingPlan::NanoGptClaude))
        }
        ChatCompletionSource::Custom => {
            resolve_custom_claude_prompt_caching_plan(endpoint_path, config, hints)
        }
        _ => Ok(None),
    }
}

pub(super) fn is_nanogpt_claude_model_name(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.starts_with("claude-")
        || model.starts_with("claude_")
        || model.contains("/claude-")
        || model.contains("/claude_")
}

fn resolve_custom_claude_prompt_caching_plan(
    endpoint_path: &str,
    config: &ChatCompletionApiConfig,
    hints: PromptCachingRequestHints,
) -> Result<Option<PromptCachingPlan>, ApplicationError> {
    if !hints.custom_claude_prompt_caching {
        return Ok(None);
    }

    if endpoint_path != "/messages" {
        return Err(ApplicationError::ValidationError(
            "Custom Claude prompt caching requires custom_api_format=claude_messages".to_string(),
        ));
    }

    let scope = custom_prompt_cache_scope(&config.base_url);
    Ok(Some(PromptCachingPlan::Claude {
        key: PromptCacheKey::CustomClaudeMessages { scope },
        anthropic_beta_header_mode: AnthropicBetaHeaderMode::PromptCachingOnly,
    }))
}

fn vertexai_claude_model_id(endpoint_path: &str) -> Option<&str> {
    let endpoint = endpoint_path.trim().trim_matches('/');
    let rest = endpoint.strip_prefix("publishers/anthropic/models/")?;
    let (model, method) = rest.rsplit_once(':')?;
    let method = method.to_ascii_lowercase();
    if model.is_empty() || (method != "rawpredict" && method != "streamrawpredict") {
        return None;
    }

    Some(model)
}

fn vertexai_claude_prompt_cache_scope(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
) -> String {
    let base_url = config.base_url.trim().trim_end_matches('/');
    let endpoint_path = endpoint_path.trim();
    let digest = Sha256::digest(format!("{base_url}|{endpoint_path}").as_bytes());
    hex_lower(&digest)
}

fn is_openrouter_claude_model(payload: &Value) -> bool {
    payload
        .as_object()
        .and_then(|object| object.get("model"))
        .and_then(Value::as_str)
        .is_some_and(is_openrouter_claude_model_name)
}

fn is_nanogpt_claude_payload(payload: &Value) -> bool {
    payload
        .as_object()
        .and_then(|object| object.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(is_nanogpt_claude_model_name)
}

fn custom_prompt_cache_scope(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/');
    let digest = Sha256::digest(normalized.as_bytes());
    hex_lower(&digest)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        PromptCachingPlan, PromptCachingRequestHints, custom_prompt_cache_scope,
        resolve_prompt_caching_plan,
    };
    use serde_json::{Map, json};
    use tt_ports::repositories::chat_completion_repository::{
        AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionSource,
    };
    use tt_ports::repositories::prompt_cache_repository::PromptCacheKey;

    fn custom_config(base_url: &str) -> ChatCompletionApiConfig {
        ChatCompletionApiConfig {
            base_url: base_url.to_string(),
            api_key: String::new(),
            authorization_header: None,
            vertexai_service_account_json: None,
            extra_headers: HashMap::new(),
            additional_headers: HashMap::new(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        }
    }

    #[test]
    fn parses_custom_prompt_caching_hint() {
        let payload = Map::from_iter([("custom_claude_prompt_caching".to_string(), json!(true))]);

        let hints = PromptCachingRequestHints::from_payload(&payload).expect("hint should parse");
        assert!(hints.custom_claude_prompt_caching);
    }

    #[test]
    fn custom_claude_prompt_caching_requires_messages_endpoint() {
        let config = custom_config("https://example.com/v1");
        let hints = PromptCachingRequestHints {
            custom_claude_prompt_caching: true,
        };

        let error = resolve_prompt_caching_plan(
            ChatCompletionSource::Custom,
            "/chat/completions",
            &config,
            &json!({}),
            hints,
        )
        .expect_err("non-Claude custom endpoint should fail");

        assert!(
            error
                .to_string()
                .contains("custom_api_format=claude_messages")
        );
    }

    #[test]
    fn custom_claude_prompt_caching_uses_scoped_storage_key() {
        let base_url = "https://example.com/v1/";
        let config = custom_config(base_url);
        let hints = PromptCachingRequestHints {
            custom_claude_prompt_caching: true,
        };

        let plan = resolve_prompt_caching_plan(
            ChatCompletionSource::Custom,
            "/messages",
            &config,
            &json!({}),
            hints,
        )
        .expect("resolution should succeed")
        .expect("plan should exist");

        assert_eq!(
            plan,
            PromptCachingPlan::Claude {
                key: PromptCacheKey::CustomClaudeMessages {
                    scope: custom_prompt_cache_scope(base_url),
                },
                anthropic_beta_header_mode: AnthropicBetaHeaderMode::PromptCachingOnly,
            }
        );
    }

    #[test]
    fn vertexai_claude_raw_predict_uses_vertex_strategy() {
        let config = custom_config(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/us-central1",
        );

        let plan = resolve_prompt_caching_plan(
            ChatCompletionSource::VertexAi,
            "/publishers/anthropic/models/claude-sonnet-4-5@20250929:rawPredict",
            &config,
            &json!({}),
            PromptCachingRequestHints::default(),
        )
        .expect("resolution should succeed")
        .expect("plan should exist");

        match plan {
            PromptCachingPlan::VertexClaude {
                key: PromptCacheKey::VertexAiClaude { scope },
                model,
            } => {
                assert_eq!(model, "claude-sonnet-4-5@20250929");
                assert!(!scope.is_empty());
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn vertexai_gemini_generate_content_does_not_use_claude_strategy() {
        let config = custom_config("https://us-central1-aiplatform.googleapis.com/v1");

        let plan = resolve_prompt_caching_plan(
            ChatCompletionSource::VertexAi,
            "/generateContent",
            &config,
            &json!({
                "model": "gemini-2.5-pro"
            }),
            PromptCachingRequestHints::default(),
        )
        .expect("resolution should succeed");

        assert_eq!(plan, None);
    }

    #[test]
    fn openrouter_claude_models_use_openrouter_strategy() {
        let config = custom_config("https://openrouter.ai/api/v1");

        let plan = resolve_prompt_caching_plan(
            ChatCompletionSource::OpenRouter,
            "/chat/completions",
            &config,
            &json!({
                "model": "anthropic/claude-sonnet-4-5"
            }),
            PromptCachingRequestHints::default(),
        )
        .expect("resolution should succeed");

        assert_eq!(
            plan,
            Some(PromptCachingPlan::OpenRouterClaude {
                key: PromptCacheKey::OpenRouterClaude,
            })
        );
    }
}
