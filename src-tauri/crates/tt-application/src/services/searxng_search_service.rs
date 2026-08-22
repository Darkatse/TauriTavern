use std::sync::Arc;

use crate::dto::searxng_search_dto::SearxngSearchRequestDto;
use crate::errors::ApplicationError;
use tt_domain::models::endpoint_url::parse_user_http_endpoint;
use tt_ports::repositories::searxng_search_repository::{
    SearxngSearchRepository, SearxngSearchRequest,
};

pub struct SearxngSearchService {
    repository: Arc<dyn SearxngSearchRepository>,
}

impl SearxngSearchService {
    pub fn new(repository: Arc<dyn SearxngSearchRepository>) -> Self {
        Self { repository }
    }

    pub fn prepare_request(
        &self,
        dto: SearxngSearchRequestDto,
    ) -> Result<SearxngSearchRequest, ApplicationError> {
        prepare_request(dto)
    }

    pub async fn search(&self, request: SearxngSearchRequest) -> Result<String, ApplicationError> {
        Ok(self.repository.search(request).await?)
    }
}

fn prepare_request(dto: SearxngSearchRequestDto) -> Result<SearxngSearchRequest, ApplicationError> {
    let base_url = parse_user_http_endpoint(&dto.base_url)?;
    if base_url.path() != "/" {
        return Err(ApplicationError::ValidationError(
            "SearXNG base URL must not include a path".to_string(),
        ));
    }

    let query = dto.query.trim().to_string();
    if query.is_empty() {
        return Err(ApplicationError::ValidationError(
            "SearXNG query is required".to_string(),
        ));
    }

    Ok(SearxngSearchRequest {
        base_url,
        query,
        preferences: optional_text(dto.preferences),
        categories: optional_text(dto.categories),
    })
}

fn optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto(base_url: &str, query: &str) -> SearxngSearchRequestDto {
        SearxngSearchRequestDto {
            base_url: base_url.to_string(),
            query: query.to_string(),
            preferences: None,
            categories: None,
        }
    }

    #[test]
    fn request_rejects_a_base_path_or_blank_query() {
        assert!(prepare_request(dto("http://localhost:8888/searxng", "rust")).is_err());
        assert!(prepare_request(dto("http://localhost:8888", " ")).is_err());
    }
}
