use tt_ports::repositories::cors_proxy_repository::Headers;

/// A request the page addressed to `/proxy/<url>`.
#[derive(Debug, Clone)]
pub struct CorsProxyRequestDto {
    pub method: String,
    pub url: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}
