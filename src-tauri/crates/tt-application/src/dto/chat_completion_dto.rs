use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tt_domain::models::upstream_failure::UpstreamFailure;

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatCompletionStreamEventDto {
    Chunk {
        seq: u64,
        data: String,
    },
    Done {
        seq: u64,
    },
    Error {
        seq: u64,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<UpstreamFailure>,
    },
}

impl ChatCompletionStreamEventDto {
    pub(crate) fn seq(&self) -> u64 {
        match self {
            Self::Chunk { seq, .. } | Self::Done { seq } | Self::Error { seq, .. } => *seq,
        }
    }

    pub(crate) fn buffered_bytes(&self) -> usize {
        match self {
            Self::Chunk { data, .. } => data.len(),
            Self::Done { .. } | Self::Error { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatCompletionStreamStatusDto {
    Running,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionStreamReadResultDto {
    pub events: Vec<ChatCompletionStreamEventDto>,
    pub status: ChatCompletionStreamStatusDto,
}
