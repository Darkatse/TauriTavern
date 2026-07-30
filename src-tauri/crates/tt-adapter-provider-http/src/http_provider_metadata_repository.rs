use std::sync::Arc;

use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Map, Value};
use url::Url;

use crate::workers_ai_models::{fetch_workers_ai_models, workers_ai_model_name};
use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_ports::repositories::provider_metadata_repository::{
    NanoGptCredits, NanoGptModelProviders, NanoGptSubscriptionCredits, NanoGptSubscriptionLimits,
    NanoGptSubscriptionPeriod, NanoGptUsageBucket, OpenRouterCredits, ProviderMetadataRepository,
    SiliconFlowEndpoint,
};

const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
const NANOGPT_API_BASE: &str = "https://nano-gpt.com/api";
const SILICONFLOW_API_BASE: &str = "https://api.siliconflow.com/v1";
const SILICONFLOW_API_BASE_CN: &str = "https://api.siliconflow.cn/v1";

pub struct HttpProviderMetadataRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpProviderMetadataRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    fn client(&self) -> Result<Client, DomainError> {
        self.http_clients
            .client(HttpClientProfile::ProviderMetadata)
    }

    fn path_segment(value: &str) -> String {
        utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
    }

    async fn send_json(
        request: RequestBuilder,
        provider_name: &str,
        action: &str,
    ) -> Result<Value, DomainError> {
        let response = request
            .send()
            .await
            .map_err(|error| DomainError::transient(format!("{action} request failed: {error}")))?;

        if !response.status().is_success() {
            return Err(Self::map_error_response(provider_name, response, action).await);
        }

        response.json::<Value>().await.map_err(|error| {
            DomainError::InternalError(format!("{action} JSON is invalid: {error}"))
        })
    }

    async fn send_advisory_json(
        request: RequestBuilder,
        provider_name: &str,
        action: &str,
    ) -> Result<Option<Value>, DomainError> {
        let response = request
            .send()
            .await
            .map_err(|error| DomainError::transient(format!("{action} request failed: {error}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let message = extract_error_message(&body, action);
            tracing::warn!(
                "{provider_name} {action} returned status {}: {message}",
                status.as_u16()
            );
            return Ok(None);
        }

        response.json::<Value>().await.map(Some).map_err(|error| {
            DomainError::InternalError(format!("{action} JSON is invalid: {error}"))
        })
    }

    async fn map_error_response(
        provider_name: &str,
        response: reqwest::Response,
        action: &str,
    ) -> DomainError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = extract_error_message(&body, action);

        match status {
            StatusCode::BAD_REQUEST => DomainError::InvalidData(message),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                DomainError::AuthenticationError(message)
            }
            StatusCode::TOO_MANY_REQUESTS => DomainError::rate_limited(format!(
                "{provider_name} {action} failed with status {}: {message}",
                status.as_u16()
            )),
            status if is_retryable_status(status) => DomainError::transient(format!(
                "{provider_name} {action} failed with status {}: {message}",
                status.as_u16()
            )),
            _ => DomainError::InternalError(format!(
                "{provider_name} {action} failed with status {}: {message}",
                status.as_u16()
            )),
        }
    }

    async fn nanogpt_balance(&self, api_key: &str) -> Result<Value, DomainError> {
        let client = self.client()?;
        let request = client
            .post(format!("{NANOGPT_API_BASE}/check-balance"))
            .header(ACCEPT, "application/json")
            .header("x-api-key", api_key);

        Self::send_json(request, "NanoGPT", "balance").await
    }

    async fn nanogpt_subscription_usage(&self, api_key: &str) -> Result<Value, DomainError> {
        let client = self.client()?;
        let request = client
            .get(format!("{NANOGPT_API_BASE}/subscription/v1/usage"))
            .header(ACCEPT, "application/json")
            .header("x-api-key", api_key);

        Self::send_json(request, "NanoGPT", "subscription usage").await
    }

    async fn workers_ai_models(
        &self,
        api_key: &str,
        account_id: &str,
        task: &str,
        per_page: u16,
    ) -> Result<Vec<Value>, DomainError> {
        let client = self.client()?;
        fetch_workers_ai_models(&client, api_key, account_id, task, per_page)
            .await?
            .into_iter()
            .map(add_workers_ai_model_id)
            .collect()
    }
}

#[async_trait]
impl ProviderMetadataRepository for HttpProviderMetadataRepository {
    async fn openrouter_model_providers(&self, model: &str) -> Result<Vec<String>, DomainError> {
        let model_path = model.trim().trim_matches('/');
        let client = self.client()?;
        let url = Url::parse(&format!(
            "{OPENROUTER_API_BASE}/models/{model_path}/endpoints"
        ))
        .map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid OpenRouter model id: {model_path}: {error}"
            ))
        })?;
        let request = client.get(url).header(ACCEPT, "application/json");

        let Some(body) = Self::send_advisory_json(request, "OpenRouter", "model providers").await?
        else {
            return Ok(Vec::new());
        };
        parse_openrouter_model_providers(&body)
    }

    async fn openrouter_credits(&self, api_key: &str) -> Result<OpenRouterCredits, DomainError> {
        let client = self.client()?;
        let request = client
            .get(format!("{OPENROUTER_API_BASE}/credits"))
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {api_key}"));

        let body = Self::send_json(request, "OpenRouter", "credits").await?;
        parse_openrouter_credits(&body)
    }

    async fn nanogpt_model_providers(
        &self,
        model: &str,
    ) -> Result<NanoGptModelProviders, DomainError> {
        let encoded_model = Self::path_segment(model);
        let client = self.client()?;
        let request = client
            .get(format!(
                "{NANOGPT_API_BASE}/models/{encoded_model}/providers"
            ))
            .header(ACCEPT, "application/json");

        let Some(body) = Self::send_advisory_json(request, "NanoGPT", "model providers").await?
        else {
            return Ok(NanoGptModelProviders {
                supports_provider_selection: false,
                providers: Vec::new(),
            });
        };
        parse_nanogpt_model_providers(&body)
    }

    async fn nanogpt_credits(&self, api_key: &str) -> Result<NanoGptCredits, DomainError> {
        let (balance, subscription) = tokio::join!(
            self.nanogpt_balance(api_key),
            self.nanogpt_subscription_usage(api_key)
        );
        let balance = balance?;
        let subscription = match subscription {
            Ok(subscription) => Some(subscription),
            Err(error) => {
                tracing::warn!("NanoGPT subscription usage request failed: {error}");
                None
            }
        };

        parse_nanogpt_credits(&balance, subscription.as_ref())
    }

    async fn siliconflow_embedding_models(
        &self,
        api_key: &str,
        endpoint: SiliconFlowEndpoint,
    ) -> Result<Vec<Value>, DomainError> {
        let base_url = match endpoint {
            SiliconFlowEndpoint::Global => SILICONFLOW_API_BASE,
            SiliconFlowEndpoint::China => SILICONFLOW_API_BASE_CN,
        };
        let client = self.client()?;
        let request = client
            .get(format!("{base_url}/models?type=text&sub_type=embedding"))
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {api_key}"));

        let body = Self::send_json(request, "SiliconFlow", "embedding models").await?;
        parse_data_array(body, "SiliconFlow embedding models")
    }

    async fn workers_ai_embedding_models(
        &self,
        api_key: &str,
        account_id: &str,
    ) -> Result<Vec<Value>, DomainError> {
        self.workers_ai_models(api_key, account_id, "Text Embeddings", 100)
            .await
    }

    async fn workers_ai_text_generation_models(
        &self,
        api_key: &str,
        account_id: &str,
    ) -> Result<Vec<Value>, DomainError> {
        self.workers_ai_models(api_key, account_id, "Text Generation", 1000)
            .await
    }

    async fn workers_ai_multimodal_models(
        &self,
        api_key: &str,
        account_id: &str,
    ) -> Result<Vec<String>, DomainError> {
        let models = self
            .workers_ai_text_generation_models(api_key, account_id)
            .await?;
        models
            .iter()
            .filter(|model| model_has_vision_property(model))
            .map(|model| required_string_field(model, "name"))
            .collect()
    }
}

fn parse_openrouter_model_providers(body: &Value) -> Result<Vec<String>, DomainError> {
    let Some(endpoints) = body
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("endpoints"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    Ok(endpoints
        .iter()
        .filter_map(|endpoint| advisory_string_field(endpoint, "provider_name"))
        .collect())
}

fn parse_openrouter_credits(body: &Value) -> Result<OpenRouterCredits, DomainError> {
    let data = object_field(body, "data", "OpenRouter credits")?;
    let total_credits = required_number_field(data, "total_credits")?;
    let total_usage = required_number_field(data, "total_usage")?;

    Ok(OpenRouterCredits {
        remaining: total_credits - total_usage,
        total_credits,
        total_usage,
    })
}

fn parse_nanogpt_model_providers(body: &Value) -> Result<NanoGptModelProviders, DomainError> {
    let Some(object) = body.as_object() else {
        return Ok(NanoGptModelProviders {
            supports_provider_selection: false,
            providers: Vec::new(),
        });
    };

    let supports_provider_selection = object
        .get("supportsProviderSelection")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let providers = object
        .get("providers")
        .and_then(Value::as_array)
        .map(|providers| {
            providers
                .iter()
                .filter(|provider| {
                    provider
                        .get("available")
                        .and_then(Value::as_bool)
                        .is_none_or(|available| available)
                })
                .filter_map(|provider| advisory_string_field(provider, "provider"))
                .collect()
        })
        .unwrap_or_default();

    Ok(NanoGptModelProviders {
        supports_provider_selection,
        providers,
    })
}

fn parse_nanogpt_credits(
    balance: &Value,
    subscription_usage: Option<&Value>,
) -> Result<NanoGptCredits, DomainError> {
    let balance = require_object(balance, "NanoGPT balance")?;
    Ok(NanoGptCredits {
        usd_balance: required_number_field(balance, "usd_balance")?,
        nano_balance: required_number_field(balance, "nano_balance")?,
        subscription: subscription_usage.and_then(parse_nanogpt_subscription),
    })
}

fn parse_nanogpt_subscription(body: &Value) -> Option<NanoGptSubscriptionCredits> {
    let body = body.as_object()?;
    let active = body.get("active").and_then(Value::as_bool).unwrap_or(false);
    if !active {
        return None;
    }

    let period = body.get("period").and_then(Value::as_object);
    let limits = body.get("limits").and_then(Value::as_object);

    Some(NanoGptSubscriptionCredits {
        active,
        state: optional_scalar_string_field(body, "state"),
        allow_overage: body
            .get("allowOverage")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        period: NanoGptSubscriptionPeriod {
            current_period_end: period
                .map(|period| optional_scalar_string_field(period, "currentPeriodEnd"))
                .unwrap_or_default(),
        },
        limits: NanoGptSubscriptionLimits {
            weekly_input_tokens: limits
                .map(|limits| optional_number_field(limits, "weeklyInputTokens"))
                .unwrap_or_default(),
            daily_input_tokens: limits
                .map(|limits| optional_number_field(limits, "dailyInputTokens"))
                .unwrap_or_default(),
            daily_images: limits
                .map(|limits| optional_number_field(limits, "dailyImages"))
                .unwrap_or_default(),
        },
        weekly_tokens: optional_usage_bucket(body, "weeklyInputTokens"),
        daily_tokens: optional_usage_bucket(body, "dailyInputTokens"),
        daily_images: optional_usage_bucket(body, "dailyImages"),
    })
}

fn optional_usage_bucket(
    object: &Map<String, Value>,
    field_name: &str,
) -> Option<NanoGptUsageBucket> {
    let object = object.get(field_name)?.as_object()?;
    Some(NanoGptUsageBucket {
        used: optional_number_field(object, "used"),
        remaining: optional_number_field(object, "remaining"),
        percent_used: optional_number_field(object, "percentUsed"),
        reset_at: optional_number_field(object, "resetAt"),
    })
}

fn add_workers_ai_model_id(model: Value) -> Result<Value, DomainError> {
    let name = workers_ai_model_name(&model)?.to_string();
    let mut object = require_object(&model, "Workers AI model")?.clone();
    object.insert("id".to_string(), Value::String(name));
    Ok(Value::Object(object))
}

fn parse_data_array(body: Value, label: &str) -> Result<Vec<Value>, DomainError> {
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response(format!("{label} data is not an array")))?;
    Ok(data.clone())
}

fn model_has_vision_property(model: &Value) -> bool {
    model
        .get("properties")
        .and_then(Value::as_array)
        .is_some_and(|properties| {
            properties.iter().any(|property| {
                property
                    .get("property_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "vision")
                    && property
                        .get("value")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == "true")
            })
        })
}

fn require_object<'a>(
    value: &'a Value,
    label: &str,
) -> Result<&'a Map<String, Value>, DomainError> {
    value
        .as_object()
        .ok_or_else(|| invalid_response(format!("{label} is not an object")))
}

fn object_field<'a>(
    object: &'a Value,
    field_name: &str,
    label: &str,
) -> Result<&'a Map<String, Value>, DomainError> {
    object
        .get(field_name)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_response(format!("{label}.{field_name} is not an object")))
}

fn advisory_string_field(value: &Value, field_name: &str) -> Option<String> {
    value
        .get(field_name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn required_string_field(value: &Value, field_name: &str) -> Result<String, DomainError> {
    let object = require_object(value, field_name)?;
    required_string_map_field(object, field_name)
}

fn required_string_map_field(
    object: &Map<String, Value>,
    field_name: &str,
) -> Result<String, DomainError> {
    object
        .get(field_name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| invalid_response(format!("{field_name} is missing")))
}

fn optional_scalar_string_field(object: &Map<String, Value>, field_name: &str) -> String {
    object
        .get(field_name)
        .and_then(scalar_to_string)
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn required_number_field(
    object: &Map<String, Value>,
    field_name: &str,
) -> Result<f64, DomainError> {
    object
        .get(field_name)
        .and_then(number_from_value)
        .ok_or_else(|| invalid_response(format!("{field_name} is not a number")))
}

fn optional_number_field(object: &Map<String, Value>, field_name: &str) -> f64 {
    object
        .get(field_name)
        .and_then(number_from_value)
        .unwrap_or_default()
}

fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn number_from_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.parse::<f64>().ok(),
        _ => None,
    }
}

fn extract_error_message(body: &str, fallback: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        for pointer in ["/error/message", "/error", "/message", "/errors/0/message"] {
            if let Some(message) = value.pointer(pointer).and_then(Value::as_str) {
                let message = message.trim();
                if !message.is_empty() {
                    return message.to_string();
                }
            }
        }
    }

    let body = body.trim();
    if body.is_empty() {
        fallback.to_string()
    } else {
        body.to_string()
    }
}

fn invalid_response(message: impl Into<String>) -> DomainError {
    DomainError::InternalError(format!("Invalid provider response: {}", message.into()))
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 500 | 502 | 503 | 504)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        add_workers_ai_model_id, model_has_vision_property, parse_nanogpt_credits,
        parse_nanogpt_model_providers, parse_openrouter_credits, parse_openrouter_model_providers,
    };

    #[test]
    fn parses_openrouter_credits() {
        let credits = parse_openrouter_credits(&json!({
            "data": { "total_credits": 10, "total_usage": 2.5 }
        }))
        .unwrap();

        assert_eq!(credits.remaining, 7.5);
    }

    #[test]
    fn openrouter_model_providers_defaults_missing_advisory_shape() {
        let providers = parse_openrouter_model_providers(&json!({
            "data": {}
        }))
        .unwrap();

        assert!(providers.is_empty());
    }

    #[test]
    fn nanogpt_model_providers_filters_unavailable_entries() {
        let providers = parse_nanogpt_model_providers(&json!({
            "supportsProviderSelection": true,
            "providers": [
                { "provider": "a", "available": true },
                { "provider": "b", "available": false },
                { "provider": "c" }
            ]
        }))
        .unwrap();

        assert_eq!(providers.providers, vec!["a", "c"]);
    }

    #[test]
    fn nanogpt_model_providers_defaults_missing_advisory_shape() {
        let providers = parse_nanogpt_model_providers(&json!({})).unwrap();

        assert!(!providers.supports_provider_selection);
        assert!(providers.providers.is_empty());
    }

    #[test]
    fn nanogpt_credits_allows_missing_subscription_usage() {
        let credits = parse_nanogpt_credits(
            &json!({
                "usd_balance": 12.5,
                "nano_balance": "3.75"
            }),
            None,
        )
        .unwrap();

        assert_eq!(credits.usd_balance, 12.5);
        assert_eq!(credits.nano_balance, 3.75);
        assert!(credits.subscription.is_none());
    }

    #[test]
    fn nanogpt_credits_treats_missing_subscription_active_as_inactive() {
        let credits = parse_nanogpt_credits(
            &json!({
                "usd_balance": 1,
                "nano_balance": 0
            }),
            Some(&json!({})),
        )
        .unwrap();

        assert!(credits.subscription.is_none());
    }

    #[test]
    fn nanogpt_credits_normalizes_partial_active_subscription() {
        let credits = parse_nanogpt_credits(
            &json!({
                "usd_balance": 1,
                "nano_balance": 0
            }),
            Some(&json!({
                "active": true,
                "weeklyInputTokens": {
                    "used": "7"
                }
            })),
        )
        .unwrap();
        let subscription = credits.subscription.expect("subscription");

        assert!(subscription.active);
        assert_eq!(subscription.state, "");
        assert!(!subscription.allow_overage);
        assert_eq!(subscription.period.current_period_end, "");
        assert_eq!(subscription.limits.weekly_input_tokens, 0.0);
        assert_eq!(subscription.limits.daily_input_tokens, 0.0);
        assert_eq!(subscription.limits.daily_images, 0.0);
        assert_eq!(subscription.weekly_tokens.unwrap().used, 7.0);
    }

    #[test]
    fn workers_ai_models_add_id_from_name() {
        let model =
            add_workers_ai_model_id(json!({ "name": "@cf/meta/llama", "properties": [] })).unwrap();

        assert_eq!(
            model.get("id").and_then(|value| value.as_str()),
            Some("@cf/meta/llama")
        );
    }

    #[test]
    fn workers_ai_vision_property_matches_upstream_shape() {
        assert!(model_has_vision_property(&json!({
            "properties": [{ "property_id": "vision", "value": "true" }]
        })));
        assert!(!model_has_vision_property(&json!({
            "properties": [{ "property_id": "vision", "value": "false" }]
        })));
    }
}
