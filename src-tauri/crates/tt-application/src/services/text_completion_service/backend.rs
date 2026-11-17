//! What differs between text completion backends. Parameter names and stream parsing belong to the
//! page.

use serde_json::{Map, Value, json};

use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::text_completion_repository::{
    TextCompletionStreamFormat, TextCompletionType,
};

use super::ollama;
use crate::dto::text_completion_dto::TextCompletionTokensDto;
use crate::errors::ApplicationError;

/// Request fields the page adds for TauriTavern; backends never receive them.
const HOST_FIELDS: &[&str] = &["type", "api_type", "api_server", "secret_id"];

/// https://platform.openai.com/docs/api-reference/completions
const OPENAI_KEYS: &[&str] = &[
    "model",
    "prompt",
    "stream",
    "temperature",
    "top_p",
    "frequency_penalty",
    "presence_penalty",
    "stop",
    "seed",
    "logit_bias",
    "logprobs",
    "max_tokens",
    "n",
    "best_of",
];
const OPENAI_MAX_STOP_SEQUENCES: usize = 4;

const VLLM_KEYS: &[&str] = &[
    "model",
    "prompt",
    "best_of",
    "echo",
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "max_tokens",
    "n",
    "presence_penalty",
    "seed",
    "stop",
    "stream",
    "suffix",
    "temperature",
    "top_p",
    "user",
    "use_beam_search",
    "top_k",
    "min_p",
    "repetition_penalty",
    "length_penalty",
    "early_stopping",
    "stop_token_ids",
    "ignore_eos",
    "min_tokens",
    "skip_special_tokens",
    "spaces_between_special_tokens",
    "truncate_prompt_tokens",
    "include_stop_str_in_output",
    "response_format",
    "guided_json",
    "guided_regex",
    "guided_choice",
    "guided_grammar",
    "guided_decoding_backend",
    "guided_whitespace_pattern",
];

pub(super) fn models_path(api_type: TextCompletionType) -> &'static str {
    match api_type {
        TextCompletionType::Tabby => "/v1/model/list",
        TextCompletionType::Ollama => "/api/tags",
        TextCompletionType::HuggingFace => "/info",
        TextCompletionType::Ooba
        | TextCompletionType::Vllm
        | TextCompletionType::Aphrodite
        | TextCompletionType::KoboldCpp
        | TextCompletionType::LlamaCpp
        | TextCompletionType::Generic => "/v1/models",
    }
}

pub(super) fn generate_path(api_type: TextCompletionType) -> &'static str {
    match api_type {
        TextCompletionType::LlamaCpp => "/completion",
        TextCompletionType::Ollama => "/api/generate",
        TextCompletionType::Ooba
        | TextCompletionType::Vllm
        | TextCompletionType::Aphrodite
        | TextCompletionType::Tabby
        | TextCompletionType::KoboldCpp
        | TextCompletionType::HuggingFace
        | TextCompletionType::Generic => "/v1/completions",
    }
}

pub(super) fn stream_format(api_type: TextCompletionType) -> TextCompletionStreamFormat {
    match api_type {
        TextCompletionType::Ollama => TextCompletionStreamFormat::OllamaNdjson,
        TextCompletionType::Ooba
        | TextCompletionType::Vllm
        | TextCompletionType::Aphrodite
        | TextCompletionType::Tabby
        | TextCompletionType::KoboldCpp
        | TextCompletionType::LlamaCpp
        | TextCompletionType::HuggingFace
        | TextCompletionType::Generic => TextCompletionStreamFormat::Sse,
    }
}

/// KoboldCpp finishes a generation whose connection closed unless it is told to stop.
pub(super) fn abort_path(api_type: TextCompletionType) -> Option<&'static str> {
    match api_type {
        TextCompletionType::KoboldCpp => Some("/api/extra/abort"),
        TextCompletionType::Ooba
        | TextCompletionType::Vllm
        | TextCompletionType::Aphrodite
        | TextCompletionType::Tabby
        | TextCompletionType::LlamaCpp
        | TextCompletionType::Ollama
        | TextCompletionType::HuggingFace
        | TextCompletionType::Generic => None,
    }
}

pub(super) fn secret_key(api_type: TextCompletionType) -> Option<&'static str> {
    match api_type {
        TextCompletionType::Ooba => Some(SecretKeys::OOBA),
        TextCompletionType::Vllm => Some(SecretKeys::VLLM),
        TextCompletionType::Aphrodite => Some(SecretKeys::APHRODITE),
        TextCompletionType::Tabby => Some(SecretKeys::TABBY),
        TextCompletionType::KoboldCpp => Some(SecretKeys::KOBOLDCPP),
        TextCompletionType::LlamaCpp => Some(SecretKeys::LLAMACPP),
        TextCompletionType::HuggingFace => Some(SecretKeys::HUGGINGFACE),
        TextCompletionType::Generic => Some(SecretKeys::GENERIC),
        TextCompletionType::Ollama => None,
    }
}

pub(super) fn auth_headers(api_type: TextCompletionType, api_key: &str) -> Vec<(String, String)> {
    let bearer = ("Authorization".to_string(), format!("Bearer {api_key}"));
    match api_type {
        TextCompletionType::Aphrodite => {
            vec![("X-API-KEY".to_string(), api_key.to_string()), bearer]
        }
        TextCompletionType::Tabby => vec![("x-api-key".to_string(), api_key.to_string()), bearer],
        TextCompletionType::Ollama => Vec::new(),
        TextCompletionType::Ooba
        | TextCompletionType::Vllm
        | TextCompletionType::KoboldCpp
        | TextCompletionType::LlamaCpp
        | TextCompletionType::HuggingFace
        | TextCompletionType::Generic => vec![bearer],
    }
}

pub(super) fn without_host_fields(mut payload: Map<String, Value>) -> Map<String, Value> {
    for field in HOST_FIELDS {
        payload.remove(*field);
    }
    payload
}

pub(super) fn generate_body(
    api_type: TextCompletionType,
    payload: Map<String, Value>,
) -> Result<Value, ApplicationError> {
    let mut payload = without_host_fields(payload);
    match api_type {
        TextCompletionType::Generic => {
            payload.retain(|key, _| OPENAI_KEYS.contains(&key.as_str()));
            if let Some(Value::Array(stop)) = payload.get_mut("stop") {
                stop.truncate(OPENAI_MAX_STOP_SEQUENCES);
            }
        }
        TextCompletionType::Vllm => payload.retain(|key, _| VLLM_KEYS.contains(&key.as_str())),
        TextCompletionType::Ollama => return ollama::generate_body(payload),
        TextCompletionType::Ooba
        | TextCompletionType::Aphrodite
        | TextCompletionType::Tabby
        | TextCompletionType::KoboldCpp
        | TextCompletionType::LlamaCpp
        | TextCompletionType::HuggingFace => {}
    }

    Ok(Value::Object(payload))
}

/// The model list in the OpenAI shape: the `data` array of `{ id, … }` objects.
pub(super) fn models(
    api_type: TextCompletionType,
    response: Value,
) -> Result<Vec<Value>, ApplicationError> {
    match api_type {
        TextCompletionType::HuggingFace => Ok(Vec::new()),
        TextCompletionType::Ollama => ollama::models(response),
        TextCompletionType::Ooba
        | TextCompletionType::Vllm
        | TextCompletionType::Aphrodite
        | TextCompletionType::Tabby
        | TextCompletionType::KoboldCpp
        | TextCompletionType::LlamaCpp
        | TextCompletionType::Generic => match response {
            Value::Object(mut response) => match response.remove("data") {
                Some(Value::Array(data)) => Ok(data),
                _ => Err(missing_model_list(api_type)),
            },
            _ => Err(missing_model_list(api_type)),
        },
    }
}

pub(super) fn encode_request(
    api_type: TextCompletionType,
    text: &str,
    model: Option<&str>,
) -> Option<(&'static str, Value)> {
    let with_model = |mut body: Value| {
        if let Some(model) = model {
            body["model"] = json!(model);
        }
        body
    };
    match api_type {
        TextCompletionType::Tabby => Some((
            "/v1/token/encode",
            json!({ "text": text, "add_bos_token": false, "encode_special_tokens": false }),
        )),
        TextCompletionType::KoboldCpp => Some((
            "/api/extra/tokencount",
            json!({ "prompt": text, "special": false }),
        )),
        TextCompletionType::LlamaCpp => Some(("/tokenize", with_model(json!({ "content": text })))),
        TextCompletionType::Vllm => Some(("/tokenize", with_model(json!({ "prompt": text })))),
        TextCompletionType::Aphrodite => {
            Some(("/v1/tokenize", with_model(json!({ "prompt": text }))))
        }
        TextCompletionType::Ooba => Some(("/v1/internal/encode", json!({ "text": text }))),
        TextCompletionType::Ollama
        | TextCompletionType::HuggingFace
        | TextCompletionType::Generic => None,
    }
}

/// Servers name the count and the ids differently; the first field present wins.
pub(super) fn encoded_tokens(
    api_type: TextCompletionType,
    response: Value,
) -> Result<TextCompletionTokensDto, ApplicationError> {
    let Value::Object(mut response) = response else {
        return Err(ApplicationError::ValidationError(format!(
            "{} returned no token count",
            api_type.display_name()
        )));
    };
    let mut take_field = |key: &str| response.remove(key).filter(|value| !value.is_null());
    let count = match ["length", "count", "value"]
        .into_iter()
        .find_map(&mut take_field)
    {
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|count| usize::try_from(count).ok())
                .ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "{} returned invalid token count",
                        api_type.display_name()
                    ))
                })?,
        ),
        None => None,
    };
    let tokens = take_field("tokens");
    let counts_tokens = tokens.is_some();
    let ids = match tokens.or_else(|| take_field("ids")) {
        Some(ids) => serde_json::from_value::<Vec<u32>>(ids).map_err(|_| {
            ApplicationError::ValidationError(format!(
                "{} returned invalid token ids",
                api_type.display_name()
            ))
        })?,
        None => Vec::new(),
    };
    let count = match count {
        Some(count) => count,
        None if counts_tokens => ids.len(),
        None => {
            return Err(ApplicationError::ValidationError(format!(
                "{} returned no token count",
                api_type.display_name()
            )));
        }
    };
    Ok(TextCompletionTokensDto { count, ids })
}

pub(super) fn missing_model_list(api_type: TextCompletionType) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "{} returned no model list",
        api_type.display_name()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_tokens_reads_each_servers_response() {
        let tokens = |response: Value| {
            encoded_tokens(TextCompletionType::LlamaCpp, response)
                .map(|tokens| (tokens.count, tokens.ids))
        };

        assert_eq!(
            tokens(json!({ "tokens": [1, 2, 3], "length": 3 })).unwrap(),
            (3, vec![1, 2, 3])
        );
        assert_eq!(
            tokens(json!({ "count": 2, "max_model_len": 8192, "tokens": [7, 8] })).unwrap(),
            (2, vec![7, 8])
        );
        assert_eq!(
            tokens(json!({ "value": 2, "ids": [4, 5] })).unwrap(),
            (2, vec![4, 5])
        );
        assert_eq!(tokens(json!({ "tokens": [9] })).unwrap(), (1, vec![9]));
        assert_eq!(
            tokens(json!({ "value": 2, "ids": null })).unwrap(),
            (2, Vec::new())
        );

        assert!(tokens(json!({ "count": -1, "tokens": [1, 2] })).is_err());
        assert!(tokens(json!({ "ids": [1] })).is_err());
        assert!(tokens(json!({ "tokens": [{ "id": 1, "piece": "a" }] })).is_err());
    }
}
