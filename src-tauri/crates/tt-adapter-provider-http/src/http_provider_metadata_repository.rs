use std::sync::Arc;

use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Map, Value, json};
use url::Url;

use tt_adapter_http::{HttpClientPool, HttpClientProfile, upstream};
use tt_domain::errors::DomainError;
use tt_ports::repositories::provider_metadata_repository::{
    ModelProvider, NanoGptCredits, NanoGptModelProviders, NanoGptSubscriptionCredits,
    NanoGptSubscriptionLimits, NanoGptSubscriptionPeriod, NanoGptUsageBucket, OpenRouterCredits,
    ProviderMetadataRepository,
};

const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
const NANOGPT_API_BASE: &str = "https://nano-gpt.com/api";

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
        let response = request.send().await.map_err(upstream::request_failure)?;

        if !response.status().is_success() {
            return Err(Self::map_error_response(provider_name, response, action).await);
        }

        serde_json::from_slice::<Value>(&upstream::read_body(response).await?).map_err(|error| {
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
}

#[async_trait]
impl ProviderMetadataRepository for HttpProviderMetadataRepository {
    async fn openrouter_providers(&self) -> Result<Vec<ModelProvider>, DomainError> {
        let client = self.client()?;
        let request = client
            .get(format!("{OPENROUTER_API_BASE}/providers"))
            .header(ACCEPT, "application/json");

        let body = Self::send_json(request, "OpenRouter", "providers").await?;
        parse_openrouter_providers(&body)
    }

    async fn openrouter_model_providers(
        &self,
        model: &str,
    ) -> Result<Vec<ModelProvider>, DomainError> {
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

        let body = Self::send_json(request, "OpenRouter", "model providers").await?;
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

    async fn exchange_openrouter_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<String, DomainError> {
        let client = self.client()?;
        let request = client
            .post(format!("{OPENROUTER_API_BASE}/auth/keys"))
            .header(ACCEPT, "application/json")
            .json(&json!({
                "code": code,
                "code_verifier": code_verifier,
                "code_challenge_method": "S256",
            }));

        let body = Self::send_json(request, "OpenRouter", "key exchange").await?;
        string_field(&body, "key")
            .ok_or_else(|| invalid_response("OpenRouter key exchange has no key"))
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

        let body = Self::send_json(request, "NanoGPT", "model providers").await?;
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
}

fn parse_openrouter_providers(body: &Value) -> Result<Vec<ModelProvider>, DomainError> {
    body.get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("OpenRouter providers are not an array"))?
        .iter()
        .map(|provider| {
            let name = required_string_field(provider, "name")?;
            Ok(ModelProvider {
                label: name.clone(),
                id: name,
            })
        })
        .collect()
}

fn parse_openrouter_model_providers(body: &Value) -> Result<Vec<ModelProvider>, DomainError> {
    let endpoints = body
        .pointer("/data/endpoints")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("OpenRouter model endpoints are not an array"))?;

    let mut providers = Vec::<ModelProvider>::new();
    for endpoint in endpoints {
        let name = required_string_field(endpoint, "provider_name")?;
        // A provider can serve one model through several endpoints.
        if providers.iter().all(|provider| provider.id != name) {
            providers.push(ModelProvider {
                label: name.clone(),
                id: name,
            });
        }
    }
    Ok(providers)
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
    if !required_bool_field(body, "supportsProviderSelection")? {
        return Ok(NanoGptModelProviders {
            supports_provider_selection: false,
            providers: Vec::new(),
        });
    }

    let entries = body
        .get("providers")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("NanoGPT model providers are not an array"))?;

    let mut providers = Vec::new();
    for entry in entries {
        if required_bool_field(entry, "available")? {
            let id = required_string_field(entry, "provider")?;
            providers.push(ModelProvider {
                label: string_field(entry, "displayName").unwrap_or_else(|| id.clone()),
                id,
            });
        }
    }
    Ok(NanoGptModelProviders {
        supports_provider_selection: true,
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

fn string_field(value: &Value, field_name: &str) -> Option<String> {
    value
        .get(field_name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn required_string_field(value: &Value, field_name: &str) -> Result<String, DomainError> {
    string_field(value, field_name)
        .ok_or_else(|| invalid_response(format!("{field_name} is not a non-empty string")))
}

fn required_bool_field(value: &Value, field_name: &str) -> Result<bool, DomainError> {
    value
        .get(field_name)
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_response(format!("{field_name} is not a boolean")))
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
        ModelProvider, NanoGptModelProviders, parse_nanogpt_credits, parse_nanogpt_model_providers,
        parse_openrouter_credits, parse_openrouter_model_providers,
    };

    #[test]
    fn parses_openrouter_credits() {
        let credits = parse_openrouter_credits(&json!({
            "data": { "total_credits": 10, "total_usage": 2.5 }
        }))
        .unwrap();

        assert_eq!(credits.remaining, 7.5);
    }

    fn provider(id: &str, label: &str) -> ModelProvider {
        ModelProvider {
            id: id.to_string(),
            label: label.to_string(),
        }
    }

    #[test]
    fn openrouter_model_providers_merge_endpoints_of_one_provider() {
        let providers = parse_openrouter_model_providers(&json!({
            "data": { "endpoints": [
                { "provider_name": "DeepInfra", "tag": "deepinfra/fp4/flex" },
                { "provider_name": "Venice", "tag": "venice/fp4" },
                { "provider_name": "DeepInfra", "tag": "deepinfra/fp4" }
            ] }
        }))
        .unwrap();

        assert_eq!(
            providers,
            vec![
                provider("DeepInfra", "DeepInfra"),
                provider("Venice", "Venice")
            ]
        );
    }

    #[test]
    fn nanogpt_model_providers_keep_available_entries() {
        let selection = parse_nanogpt_model_providers(&json!({
            "supportsProviderSelection": true,
            "providers": [
                { "provider": "sailresearch-asap", "displayName": "Sail Research (ASAP)", "available": true },
                { "provider": "novita", "displayName": "Novita", "available": false },
                { "provider": "moonshot", "available": true }
            ]
        }))
        .unwrap();

        assert_eq!(
            selection,
            NanoGptModelProviders {
                supports_provider_selection: true,
                providers: vec![
                    provider("sailresearch-asap", "Sail Research (ASAP)"),
                    provider("moonshot", "moonshot"),
                ],
            }
        );
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
}
