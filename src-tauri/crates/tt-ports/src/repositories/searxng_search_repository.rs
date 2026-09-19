use async_trait::async_trait;
use url::Url;

use tt_domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct SearxngSearchRequest {
    pub base_url: Url,
    pub query: String,
    pub preferences: Option<String>,
    pub categories: Option<String>,
}

#[async_trait]
pub trait SearxngSearchRepository: Send + Sync {
    async fn search(&self, request: SearxngSearchRequest) -> Result<String, DomainError>;
}
