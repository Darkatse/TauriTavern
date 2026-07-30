use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsRouteResponseDto {
    pub status: u16,
    pub content_type: String,
    pub body_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
}
