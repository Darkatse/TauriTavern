//! Ollama has its own generate and model list APIs. The request is rebuilt here; the adapter
//! reads its generate stream.

use serde_json::{Map, Value, json};

use tt_ports::repositories::text_completion_repository::TextCompletionType;

use super::backend::missing_model_list;
use crate::errors::ApplicationError;

/// https://github.com/ollama/ollama/blob/main/docs/api.md#request-8
const OPTION_KEYS: &[&str] = &[
    "num_predict",
    "num_ctx",
    "num_batch",
    "stop",
    "temperature",
    "repeat_penalty",
    "presence_penalty",
    "frequency_penalty",
    "top_k",
    "top_p",
    "tfs_z",
    "typical_p",
    "seed",
    "repeat_last_n",
    "min_p",
];

/// The model stays loaded between requests, as it does for embeddings.
const KEEP_ALIVE: i64 = -1;

/// The prompt is sent raw: the page has already applied the instruct template.
pub(super) fn generate_body(mut payload: Map<String, Value>) -> Result<Value, ApplicationError> {
    let options = OPTION_KEYS
        .iter()
        .filter_map(|key| payload.remove(*key).map(|value| (key.to_string(), value)))
        .collect::<Map<_, _>>();

    let stream = match payload.remove("stream") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(stream)) => stream,
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "Text completion request field must be a boolean: stream".to_string(),
            ));
        }
    };

    let mut body = Map::new();
    for key in ["model", "prompt"] {
        if let Some(value) = payload.remove(key) {
            body.insert(key.to_string(), value);
        }
    }
    body.insert("stream".to_string(), Value::Bool(stream));
    body.insert("keep_alive".to_string(), json!(KEEP_ALIVE));
    body.insert("raw".to_string(), Value::Bool(true));
    body.insert("options".to_string(), Value::Object(options));
    Ok(Value::Object(body))
}

pub(super) fn models(response: Value) -> Result<Vec<Value>, ApplicationError> {
    let Value::Object(mut response) = response else {
        return Err(missing_model_list(TextCompletionType::Ollama));
    };
    let Some(Value::Array(models)) = response.remove("models") else {
        return Err(missing_model_list(TextCompletionType::Ollama));
    };

    Ok(models
        .into_iter()
        .map(|model| match model {
            Value::Object(mut fields) => {
                if let Some(name) = fields.get("name").cloned() {
                    fields.insert("id".to_string(), name);
                }
                Value::Object(fields)
            }
            other => other,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_body_sends_a_raw_prompt_with_sampler_options() {
        let payload = json!({
            "model": "qwen3:0.6b",
            "prompt": "<|im_start|>user\nHi<|im_end|>\n",
            "stream": true,
            "num_predict": 64,
            "num_ctx": 4096,
            "temperature": 0.7,
            "stop": ["<|im_end|>"],
            "max_tokens": 64,
            "repetition_penalty": 1.1,
        });
        let Value::Object(payload) = payload else {
            unreachable!()
        };

        assert_eq!(
            generate_body(payload).unwrap(),
            json!({
                "model": "qwen3:0.6b",
                "prompt": "<|im_start|>user\nHi<|im_end|>\n",
                "stream": true,
                "keep_alive": -1,
                "raw": true,
                "options": {
                    "num_predict": 64,
                    "num_ctx": 4096,
                    "temperature": 0.7,
                    "stop": ["<|im_end|>"],
                },
            })
        );

        let Value::Object(null_stream) = json!({ "model": "m", "prompt": "p", "stream": null })
        else {
            unreachable!()
        };
        assert_eq!(generate_body(null_stream).unwrap()["stream"], json!(false));

        let Value::Object(invalid_stream) =
            json!({ "model": "m", "prompt": "p", "stream": "true" })
        else {
            unreachable!()
        };
        assert!(matches!(
            generate_body(invalid_stream),
            Err(ApplicationError::ValidationError(_))
        ));
    }
}
