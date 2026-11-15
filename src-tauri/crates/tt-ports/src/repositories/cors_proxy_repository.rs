use async_trait::async_trait;
use url::Url;

use tt_domain::errors::DomainError;

use crate::byte_reader::ByteReader;

/// Header names with their raw values, in order.
pub type Headers = Vec<(String, Vec<u8>)>;

/// A page's request for `url`, sent to the user endpoint `endpoint` that contains it.
pub struct CorsProxyRequest {
    pub endpoint: String,
    pub url: Url,
    pub method: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

pub struct CorsProxyResponse {
    pub status: u16,
    pub headers: Headers,
    /// `None` when HTTP gives the response no content.
    pub body: Option<Box<dyn ByteReader>>,
}

#[async_trait]
pub trait CorsProxyRepository: Send + Sync {
    async fn send(&self, request: CorsProxyRequest) -> Result<CorsProxyResponse, DomainError>;
}
