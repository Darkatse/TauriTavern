use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tt_contracts::observability::RequestDescriptionDto;
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatCompletionStatusRequestDto {
    #[serde(default)]
    pub chat_completion_source: String,
    #[serde(default)]
    pub custom_api_format: String,
    #[serde(default)]
    pub opencode_endpoint: String,
    #[serde(default)]
    pub opencode_api_format: String,
    #[serde(default)]
    pub reverse_proxy: String,
    #[serde(default)]
    pub proxy_password: String,
    #[serde(default)]
    pub custom_url: String,
    #[serde(default)]
    pub custom_include_headers: Value,
    #[serde(default)]
    pub siliconflow_endpoint: String,
    #[serde(default)]
    pub minimax_endpoint: String,
    #[serde(default)]
    pub moonshot_endpoint: String,
    #[serde(default)]
    pub pollinations_endpoint: String,
    #[serde(default)]
    pub workers_ai_account_id: String,
    #[serde(default)]
    pub aws_bedrock_region: String,
    #[serde(default)]
    pub secret_id: Option<String>,
    #[serde(default)]
    pub bypass_status_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatCompletionGenerateRequestDto {
    #[serde(flatten)]
    pub payload: Map<String, Value>,
}

impl ChatCompletionGenerateRequestDto {
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.payload.get(key).and_then(Value::as_str)
    }
}

#[derive(Debug)]
pub struct ChatCompletionWireRequestDto {
    pub source: ChatCompletionSource,
    pub base_url: String,
    pub endpoint_path: String,
    /// Retains the model even when the wire body moves it into the URL.
    pub payload: Value,
    pub body: Value,
    pub http: RequestDescriptionDto,
}
