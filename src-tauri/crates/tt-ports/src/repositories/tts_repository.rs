use async_trait::async_trait;
use serde_json::{Value, json};
use url::Url;

use tt_domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct GrokOutputFormat {
    pub codec: String,
    pub sample_rate: u32,
    pub bit_rate: u32,
}

#[derive(Debug, Clone)]
pub struct MinimaxGenerateRequest {
    pub api_key: String,
    pub text: String,
    pub voice_id: String,
    pub api_host: String,
    pub model: String,
    pub speed: f64,
    pub volume: f64,
    pub pitch: i64,
    pub audio_sample_rate: u32,
    pub bitrate: u32,
    pub format: String,
    pub language_boost: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AzureTtsRequest {
    List {
        api_key: String,
        region: String,
    },
    Generate {
        api_key: String,
        region: String,
        text: String,
        voice: String,
    },
}

#[derive(Debug, Clone)]
pub enum GoogleTranslateTtsRequest {
    ListVoices,
    Generate { text: Vec<String>, voice: String },
}

#[derive(Debug, Clone)]
pub enum GoogleGeminiTtsRequest {
    ListVoices,
    Generate {
        text: String,
        voice: String,
        model: String,
        base_url: String,
        api_key: String,
    },
}

#[derive(Debug, Clone)]
pub enum OpenAiTtsRequest {
    Generate {
        api_key: String,
        text: String,
        voice: String,
        model: String,
        speed: f64,
        instructions: Option<String>,
    },
    CompatibleGenerate {
        api_key: Option<String>,
        endpoint: Url,
        input: String,
        voice: String,
        model: String,
        response_format: String,
        speed: f64,
    },
}

#[derive(Debug, Clone)]
pub enum ElectronHubTtsRequest {
    Models { api_key: String },
    Generate { api_key: String, payload: Value },
}

#[derive(Debug, Clone)]
pub struct ElevenLabsVoiceFile {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum ElevenLabsTtsRequest {
    Voices {
        api_key: String,
    },
    VoiceSettings {
        api_key: String,
    },
    Synthesize {
        api_key: String,
        voice_id: String,
        request: Value,
    },
    History {
        api_key: String,
    },
    HistoryAudio {
        api_key: String,
        history_item_id: String,
    },
    AddVoice {
        api_key: String,
        name: String,
        description: String,
        labels: String,
        files: Vec<ElevenLabsVoiceFile>,
    },
}

#[derive(Debug, Clone)]
pub enum PollinationsTtsRequest {
    Voices {
        model: String,
    },
    Generate {
        api_key: String,
        text: String,
        model: String,
        voice: String,
    },
}

#[derive(Debug, Clone)]
pub struct VolcengineTtsRequest {
    pub app_id: String,
    pub access_key: String,
    pub provider_endpoint: String,
    pub resource_id: String,
    pub text: String,
    pub voice_speaker: String,
    pub speed: i64,
}

#[derive(Debug, Clone)]
pub enum TtsRequest {
    Azure(AzureTtsRequest),
    GoogleTranslate(GoogleTranslateTtsRequest),
    GoogleGemini(GoogleGeminiTtsRequest),
    NovelAiGenerate {
        api_key: String,
        text: String,
        voice: String,
    },
    OpenAi(OpenAiTtsRequest),
    ElectronHub(ElectronHubTtsRequest),
    ChutesGenerate {
        api_key: String,
        input: String,
        voice: String,
        speed: f64,
    },
    ElevenLabs(ElevenLabsTtsRequest),
    Pollinations(PollinationsTtsRequest),
    Volcengine(VolcengineTtsRequest),
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
