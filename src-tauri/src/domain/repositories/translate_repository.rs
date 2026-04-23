use async_trait::async_trait;
use url::Url;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslateProvider {
    Google,
    Libre,
    Lingva,
    Deepl,
    Deeplx,
    OneRing,
}

impl TranslateProvider {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "google" => Some(Self::Google),
            "libre" => Some(Self::Libre),
            "lingva" => Some(Self::Lingva),
            "deepl" => Some(Self::Deepl),
            "deeplx" => Some(Self::Deeplx),
            "onering" => Some(Self::OneRing),
            _ => None,
        }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeeplApiEndpoint {
    Free,
    Pro,
}

impl DeeplApiEndpoint {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "free" => Some(Self::Free),
            "pro" => Some(Self::Pro),
            _ => None,
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Self::Free => "https://api-free.deepl.com/v2/translate",
            Self::Pro => "https://api.deepl.com/v2/translate",
        }
    }
}

#[derive(Debug, Clone)]
pub enum TranslateRequest {
    Google { text: String, lang: String },
    Libre {
        url: Url,
        api_key: Option<String>,
        text: String,
        lang: String,
    },
    Lingva {
        base_url: Url,
        text: String,
        lang: String,
    },
    Deepl {
        endpoint: DeeplApiEndpoint,
        auth_key: String,
        text: String,
        lang: String,
    },
    Deeplx { url: Url, text: String, lang: String },
    OneRing {
        url: Url,
        text: String,
        from_lang: String,
        to_lang: String,
    },
}

#[async_trait]
pub trait TranslateRepository: Send + Sync {
    async fn translate(&self, request: TranslateRequest) -> Result<String, DomainError>;
}
