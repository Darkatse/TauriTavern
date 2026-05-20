use async_trait::async_trait;
use serde_json::json;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct GrokOutputFormat {
    pub codec: String,
    pub sample_rate: u32,
    pub bit_rate: u32,
}

#[derive(Debug, Clone)]
pub struct MinimaxGenerateRequest {
    pub api_key: String,
    pub group_id: String,
    pub text: String,
    pub voice_id: String,
    pub api_host: String,
    pub model: String,
    pub speed: f64,
    pub volume: f64,
    pub pitch: f64,
    pub audio_sample_rate: u32,
    pub bitrate: u32,
    pub format: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TtsRequest {
    GrokVoices {
        api_key: String,
    },
    GrokGenerate {
        api_key: String,
        text: String,
        voice_id: String,
        language: String,
        output_format: GrokOutputFormat,
    },
    MimoGenerate {
        api_key: String,
        text: String,
        voice_id: String,
        model: String,
        format: String,
        instructions: Option<String>,
    },
    MinimaxGenerate {
        request: MinimaxGenerateRequest,
    },
}

#[derive(Debug, Clone)]
pub struct TtsRouteResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub status_text: Option<String>,
}

impl TtsRouteResponse {
    pub fn bytes(status: u16, content_type: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body,
            status_text: None,
        }
    }

    pub fn text(status: u16, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            status,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: message.clone().into_bytes(),
            status_text: Some(message),
        }
    }

    pub fn json_error(status: u16, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            status,
            content_type: "application/json; charset=utf-8".to_string(),
            body: json!({ "error": message }).to_string().into_bytes(),
            status_text: None,
        }
    }
}

#[async_trait]
pub trait TtsRepository: Send + Sync {
    async fn handle(&self, request: TtsRequest) -> Result<TtsRouteResponse, DomainError>;
}
