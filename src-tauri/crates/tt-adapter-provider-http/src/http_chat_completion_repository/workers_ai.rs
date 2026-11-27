use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::ChatCompletionApiConfig;

use super::{HttpChatCompletionRepository, openai};
use crate::workers_ai_models::{parse_workers_ai_models, with_workers_ai_model_id};

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let body = openai::list_models_with_path(
        repository,
        config,
        "Cloudflare Workers AI",
        "/models/search?task=Text%20Generation&per_page=1000",
    )
    .await?;

    let data = parse_workers_ai_models(body)?
        .into_iter()
        .map(with_workers_ai_model_id)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json!({ "data": data }))
}
