use async_trait::async_trait;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct GrokOutputFormat {
    pub codec: String,
    pub sample_rate: u32,
    pub bit_rate: u32,
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
}

#[async_trait]
pub trait TtsRepository: Send + Sync {
    async fn handle(&self, request: TtsRequest) -> Result<TtsRouteResponse, DomainError>;
}
