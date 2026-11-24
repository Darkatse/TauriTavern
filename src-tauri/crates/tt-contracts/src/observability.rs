use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const COMMAND: &str = "tauritavern::command";
pub const USER_VISIBLE_ERROR: &str = "tauritavern::user_error";

pub const REDACTED: &str = "[redacted]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmApiRawKindDto {
    Json,
    Sse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiLogIndexEntryDto {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub ok: bool,
    pub source: String,
    pub model: Option<String>,
    pub endpoint: String,
    pub duration_ms: u32,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiLogPreviewDto {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub ok: bool,
    pub source: String,
    pub model: Option<String>,
    pub endpoint: String,
    pub duration_ms: u32,
    pub stream: bool,
    pub error_message: Option<String>,
    pub request_readable: String,
    pub response_readable: String,
    pub response_raw_kind: Option<LlmApiRawKindDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiLogRawDto {
    pub id: u64,
    pub request_raw: String,
    pub response_raw: String,
    pub response_raw_kind: Option<LlmApiRawKindDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiLogViewDto {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub ok: bool,
    pub source: String,
    pub model: Option<String>,
    pub endpoint: String,
    pub duration_ms: u32,
    pub stream: bool,
    pub response_raw_kind: Option<LlmApiRawKindDto>,
    pub view: ExchangeViewDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionRequestPreviewDto {
    pub source: String,
    pub model: Option<String>,
    pub endpoint: String,
    pub request_raw: String,
    pub request_readable: String,
    pub view: ExchangeViewDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKindDto {
    Chat,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeViewDto {
    pub request: RequestViewDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseViewDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestViewDto {
    pub kind: RequestKindDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<RequestDescriptionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_path: Option<String>,
    pub system: Vec<PartViewDto>,
    pub messages: Vec<MessageViewDto>,
    pub tools: Vec<ToolViewDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    pub parameters: Vec<ParameterViewDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Credentials in the URL and headers are already replaced with [`REDACTED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDescriptionDto {
    pub transport: RequestTransportDto,
    pub method: String,
    pub url: String,
    pub headers: Vec<RequestHeaderDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestTransportDto {
    Http,
    Websocket,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestHeaderDto {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseViewDto {
    pub candidates: Vec<MessageViewDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageViewDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageViewDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
}

impl UsageViewDto {
    pub fn is_empty(&self) -> bool {
        self.input.is_none()
            && self.cache_read.is_none()
            && self.output.is_none()
            && self.reasoning.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageViewDto {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub parts: Vec<PartViewDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolViewDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterViewDto {
    pub key: String,
    pub path: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PartViewDto {
    #[serde(rename_all = "camelCase")]
    Text {
        text: String,
        cache_breakpoint: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    NativeState {
        kind: String,
        length: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Media {
        mime: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoded_length: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        parts: Vec<PartViewDto>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Other {
        label: String,
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
}
