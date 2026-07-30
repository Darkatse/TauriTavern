use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Client, StatusCode};
use serde_json::Value;

use crate::workers_ai_endpoint::workers_ai_models_search_url;
use tt_domain::errors::DomainError;

pub(crate) async fn fetch_workers_ai_models(
    client: &Client,
    api_key: &str,
    account_id: &str,
    task: &str,
    per_page: u16,
) -> Result<Vec<Value>, DomainError> {
    let mut url = workers_ai_models_search_url(account_id)?;
    url.query_pairs_mut()
        .append_pair("task", task)
        .append_pair("per_page", &per_page.to_string());

    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .send()
        .await
        .map_err(|error| {
            DomainError::transient(format!(
                "Cloudflare Workers AI models request failed: {error}"
            ))
        })?;

    if !response.status().is_success() {
        return Err(map_workers_ai_models_error(response).await);
    }

    let body = response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Cloudflare Workers AI models JSON is invalid: {error}"
        ))
    })?;

    parse_workers_ai_models(&body)
}

pub(crate) fn workers_ai_model_name(model: &Value) -> Result<&str, DomainError> {
    model
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            DomainError::InternalError(
                "Cloudflare Workers AI model response contains a model without name.".to_string(),
            )
        })
}

fn parse_workers_ai_models(body: &Value) -> Result<Vec<Value>, DomainError> {
    if body.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(DomainError::InternalError(
            "Cloudflare Workers AI returned unsuccessful models response.".to_string(),
        ));
    }

    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DomainError::InternalError(
                "Cloudflare Workers AI models result is not an array.".to_string(),
            )
        })?;

    Ok(result.clone())
}

async fn map_workers_ai_models_error(response: reqwest::Response) -> DomainError {
    let status = response.status();
    let detail = response.text().await.unwrap_or_default();
    let detail = detail.trim();
    let message = if detail.is_empty() {
        format!(
            "Cloudflare Workers AI models request failed with status {}",
            status.as_u16()
        )
    } else {
        format!(
            "Cloudflare Workers AI models request failed with status {}: {detail}",
            status.as_u16()
        )
    };

    match status {
        StatusCode::BAD_REQUEST => DomainError::InvalidData(message),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            DomainError::AuthenticationError(message)
        }
        StatusCode::TOO_MANY_REQUESTS => DomainError::rate_limited(message),
        status if status.is_server_error() => DomainError::transient(message),
        _ => DomainError::InternalError(message),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse_workers_ai_models, workers_ai_model_name};

    #[test]
    fn parses_models_result_without_mutating_shape() {
        let models = parse_workers_ai_models(&json!({
            "success": true,
            "result": [{ "name": "@cf/meta/llama", "properties": [] }]
        }))
        .unwrap();

        assert_eq!(
            models[0].get("name").and_then(|value| value.as_str()),
            Some("@cf/meta/llama")
        );
        assert!(models[0].get("id").is_none());
    }

    #[test]
    fn rejects_unsuccessful_models_response() {
        let error = parse_workers_ai_models(&json!({
            "success": false,
            "result": []
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unsuccessful models response"));
    }

    #[test]
    fn rejects_models_response_without_success_flag() {
        let error = parse_workers_ai_models(&json!({
            "result": []
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unsuccessful models response"));
    }

    #[test]
    fn model_name_is_required() {
        let error = workers_ai_model_name(&json!({ "properties": [] })).unwrap_err();

        assert!(error.to_string().contains("without name"));
    }
}
