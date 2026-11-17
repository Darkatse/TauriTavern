use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextCompletionStatusRequestDto {
    #[serde(default)]
    pub api_server: String,
    #[serde(default)]
    pub api_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextCompletionStatusDto {
    /// The loaded or first listed model: `Valid` when the server lists none, `None` when
    /// TabbyAPI has none loaded.
    pub result: String,
    pub data: Vec<Value>,
    pub supports_tokenization: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextCompletionPropsRequestDto {
    #[serde(default)]
    pub api_server: String,
    #[serde(default)]
    pub api_type: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextCompletionGenerateRequestDto {
    #[serde(flatten)]
    pub payload: Map<String, Value>,
}
