use std::sync::Arc;

use url::Url;

use crate::dto::cors_proxy_dto::CorsProxyRequestDto;
use crate::errors::ApplicationError;
use tt_ports::repositories::cors_proxy_repository::{
    CorsProxyRepository, CorsProxyRequest, CorsProxyResponse,
};

/// TauriTavern's own request header, which the target never asked for.
const HOST_CSRF_HEADER: &str = "x-csrf-token";

pub struct CorsProxyService {
    repository: Arc<dyn CorsProxyRepository>,
}

impl CorsProxyService {
    pub fn new(repository: Arc<dyn CorsProxyRepository>) -> Self {
        Self { repository }
    }

    pub fn prepare_request(
        &self,
        dto: CorsProxyRequestDto,
    ) -> Result<CorsProxyRequest, ApplicationError> {
        prepare_request(dto)
    }

    pub async fn send(
        &self,
        request: CorsProxyRequest,
    ) -> Result<CorsProxyResponse, ApplicationError> {
        Ok(self.repository.send(request).await?)
    }
}

fn prepare_request(dto: CorsProxyRequestDto) -> Result<CorsProxyRequest, ApplicationError> {
    let mut url = Url::parse(&dto.url).map_err(|error| {
        ApplicationError::ValidationError(format!("Invalid CORS proxy target: {error}"))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApplicationError::ValidationError(
            "CORS proxy target must use http or https".to_string(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ApplicationError::ValidationError(
            "CORS proxy target must not include credentials".to_string(),
        ));
    }
    url.set_fragment(None);

    let headers = dto
        .headers
        .into_iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case(HOST_CSRF_HEADER))
        .collect();

    Ok(CorsProxyRequest {
        url,
        method: dto.method,
        headers,
        body: dto.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto(url: &str) -> CorsProxyRequestDto {
        CorsProxyRequestDto {
            method: "POST".to_string(),
            url: url.to_string(),
            headers: vec![
                ("x-csrf-token".to_string(), b"tauri-dummy-token".to_vec()),
                ("authorization".to_string(), b"Bearer key".to_vec()),
            ],
            body: Vec::new(),
        }
    }

    #[test]
    fn prepared_request_keeps_the_query_and_drops_the_host_token() {
        let request =
            prepare_request(dto("https://api.example.com:8443/v1/tts?format=mp3#part")).unwrap();

        assert_eq!(
            request.url.as_str(),
            "https://api.example.com:8443/v1/tts?format=mp3"
        );
        assert_eq!(
            request.headers,
            vec![("authorization".to_string(), b"Bearer key".to_vec())]
        );

        for rejected in [
            "file:///etc/passwd",
            "ftp://files.example.com/a",
            "https://user:secret@api.example.com/v1",
            "/v1/tts",
        ] {
            assert!(prepare_request(dto(rejected)).is_err(), "{rejected}");
        }
    }
}
