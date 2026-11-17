use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::{Method, RequestBuilder};
use serde_json::Value;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::append_endpoint_path;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::text_completion_repository::{
    TextCompletionEndpoint, TextCompletionRepository, TextCompletionStreamFormat,
};

use crate::provider_response;

mod ollama;

pub struct HttpTextCompletionRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpTextCompletionRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    fn request(
        &self,
        profile: HttpClientProfile,
        endpoint: &TextCompletionEndpoint,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder, DomainError> {
        let url = append_endpoint_path(&endpoint.base_url, path)?;
        let client = self
            .http_clients
            .user_endpoint_client(profile, &endpoint.base_url)?;

        Ok(endpoint
            .headers
            .iter()
            .fold(client.request(method, url), |request, (name, value)| {
                request.header(name, value)
            }))
    }

    async fn send_json(
        &self,
        profile: HttpClientProfile,
        endpoint: &TextCompletionEndpoint,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, DomainError> {
        let provider_name = endpoint.api_type.display_name();
        let mut request = self
            .request(profile, endpoint, method, path)?
            .header(ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").json(body);
        }

        let response =
            provider_response::send_checked(request, provider_name, "Request failed").await?;
        provider_response::read_upstream_json_body(provider_name, path, response).await
    }
}

#[async_trait]
impl TextCompletionRepository for HttpTextCompletionRepository {
    async fn get(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
    ) -> Result<Value, DomainError> {
        self.send_json(
            HttpClientProfile::ProviderMetadata,
            endpoint,
            Method::GET,
            path,
            None,
        )
        .await
    }

    async fn post(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError> {
        self.send_json(
            HttpClientProfile::ProviderMetadata,
            endpoint,
            Method::POST,
            path,
            Some(body),
        )
        .await
    }

    async fn download(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError> {
        self.send_json(
            HttpClientProfile::GenerationStream,
            endpoint,
            Method::POST,
            path,
            Some(body),
        )
        .await
    }

    async fn generate(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError> {
        self.send_json(
            HttpClientProfile::Generation,
            endpoint,
            Method::POST,
            path,
            Some(body),
        )
        .await
    }

    async fn generate_stream(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
        format: TextCompletionStreamFormat,
        sender: GenerationStreamSender,
        cancel: GenerationCancelReceiver,
    ) -> Result<(), DomainError> {
        let provider_name = endpoint.api_type.display_name();
        let accept = match format {
            TextCompletionStreamFormat::Sse => "text/event-stream",
            TextCompletionStreamFormat::OllamaNdjson => "application/x-ndjson",
        };
        let request = self
            .request(
                HttpClientProfile::GenerationStream,
                endpoint,
                Method::POST,
                path,
            )?
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, accept)
            .json(body);
        let response =
            provider_response::send_checked(request, provider_name, "Generation request failed")
                .await?;

        match format {
            TextCompletionStreamFormat::Sse => {
                provider_response::stream_sse_response(provider_name, response, sender, cancel)
                    .await
            }
            TextCompletionStreamFormat::OllamaNdjson => {
                ollama::stream(response, sender, cancel).await
            }
        }
    }
}
