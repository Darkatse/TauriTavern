use serde_json::{Map, Value, json};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::ChatCompletionApiConfig;

use super::{HttpChatCompletionRepository, openai};

const SERVERLESS_MODELS_PATH: &str =
    "/accounts/fireworks/models?filter=supports_serverless%3Dtrue&pageSize=200";

// Fireworks omits fast routers from its model catalog.
const FAST_ROUTERS: [(&str, &str); 4] = [
    (
        "accounts/fireworks/models/glm-5p2",
        "accounts/fireworks/routers/glm-5p2-fast",
    ),
    (
        "accounts/fireworks/models/kimi-k2p6",
        "accounts/fireworks/routers/kimi-k2p6-fast",
    ),
    (
        "accounts/fireworks/models/kimi-k2p7-code",
        "accounts/fireworks/routers/kimi-k2p7-code-fast",
    ),
    (
        "accounts/fireworks/models/kimi-k3",
        "accounts/fireworks/routers/kimi-k3-fast",
    ),
];

// `/models` omits most serverless models.
pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let body =
        openai::list_models_with_path(repository, config, "Fireworks AI", SERVERLESS_MODELS_PATH)
            .await?;

    Ok(json!({ "data": chat_models(&body)? }))
}

fn chat_models(body: &Value) -> Result<Vec<Value>, DomainError> {
    let models = body
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("models is not an array"))?;

    let mut catalog = Vec::with_capacity(models.len());
    for model in models {
        let context_length = model.get("contextLength").and_then(Value::as_u64);
        let is_embedding = model.get("kind").and_then(Value::as_str) == Some("EMBEDDING_MODEL");
        if context_length.is_none_or(|length| length == 0) || is_embedding {
            continue;
        }

        let id = model
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("model name is missing"))?;
        let mut entry = Map::new();
        entry.insert("id".to_string(), Value::String(id.to_string()));
        for (field, key) in [
            ("displayName", "name"),
            ("contextLength", "context_length"),
            ("supportsTools", "supports_tools"),
            ("supportsImageInput", "supports_image_input"),
        ] {
            if let Some(value) = model.get(field) {
                entry.insert(key.to_string(), value.clone());
            }
        }
        catalog.push(entry);
    }

    for (model_id, router_id) in FAST_ROUTERS {
        let Some(model) = catalog.iter().find(|entry| entry["id"] == model_id) else {
            continue;
        };
        let mut router = model.clone();
        router.insert("id".to_string(), Value::String(router_id.to_string()));
        if let Some(name) = model.get("name").and_then(Value::as_str) {
            router.insert("name".to_string(), Value::String(format!("{name} (fast)")));
        }
        catalog.push(router);
    }

    Ok(catalog.into_iter().map(Value::Object).collect())
}

fn invalid_response(reason: &str) -> DomainError {
    DomainError::InternalError(format!("Invalid Fireworks AI models response: {reason}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::chat_models;

    #[test]
    fn serverless_chat_models_and_their_fast_routers_are_listed() {
        let body = json!({
            "models": [
                {
                    "name": "accounts/fireworks/models/glm-5p2",
                    "displayName": "GLM 5.2",
                    "kind": "HF_BASE_MODEL",
                    "contextLength": 202752,
                    "supportsTools": true,
                    "supportsImageInput": false
                },
                {
                    "name": "accounts/fireworks/models/qwen3-embedding-8b",
                    "kind": "EMBEDDING_MODEL",
                    "contextLength": 40960
                },
                { "name": "accounts/fireworks/models/flux-1-schnell-fp8", "contextLength": 0 },
                { "name": "accounts/fireworks/models/whisper-v3" }
            ]
        });

        let glm = json!({
            "id": "accounts/fireworks/models/glm-5p2",
            "name": "GLM 5.2",
            "context_length": 202752,
            "supports_tools": true,
            "supports_image_input": false
        });
        let mut glm_fast = glm.clone();
        glm_fast["id"] = json!("accounts/fireworks/routers/glm-5p2-fast");
        glm_fast["name"] = json!("GLM 5.2 (fast)");
        assert_eq!(chat_models(&body).unwrap(), [glm, glm_fast]);
    }
}
