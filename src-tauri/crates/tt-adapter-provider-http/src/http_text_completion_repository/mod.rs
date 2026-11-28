use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_contracts::observability::{RequestDescriptionDto, RequestTransportDto};
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::append_endpoint_path;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::text_completion_repository::{
    TextCompletionEndpoint, TextCompletionRepository, TextCompletionStreamFormat,
};

use crate::provider_request::{EVENT_STREAM, JSON, ProviderRequest};
use crate::provider_response;

mod ollama;

const NDJSON: &str = "application/x-ndjson";

pub struct HttpTextCompletionRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpTextCompletionRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    async fn send(
        &self,
        profile: HttpClientProfile,
        endpoint: &TextCompletionEndpoint,
        request: ProviderRequest,
        body: Option<&Value>,
        error_context: &str,
    ) -> Result<reqwest::Response, DomainError> {
        let client = if endpoint.user_configured_endpoint {
            self.http_clients
                .user_endpoint_client(profile, &endpoint.base_url)?
        } else {
            self.http_clients.client(profile)?
        };
        let request = request.with_client(&client);
        let request = match body {
            Some(body) => request.json(body),
            None => request,
        };
        provider_response::send_checked(request, endpoint.api_type.display_name(), error_context)
            .await
    }

    async fn send_json(
        &self,
        profile: HttpClientProfile,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, DomainError> {
        let request = match body {
            Some(_) => post_request(endpoint, path, JSON),
            None => get_request(endpoint, path),
        }?;
        let response = self
            .send(profile, endpoint, request, body, "Request failed")
            .await?;
        provider_response::read_upstream_json_body(endpoint.api_type.display_name(), path, response)
            .await
    }
}

fn get_request(
    endpoint: &TextCompletionEndpoint,
    path: &str,
) -> Result<ProviderRequest, DomainError> {
    let url = append_endpoint_path(&endpoint.base_url, path)?;
    with_endpoint_headers(ProviderRequest::get_json(url), endpoint)
}

fn post_request(
    endpoint: &TextCompletionEndpoint,
    path: &str,
    accept: &'static str,
) -> Result<ProviderRequest, DomainError> {
    let url = append_endpoint_path(&endpoint.base_url, path)?;
    with_endpoint_headers(ProviderRequest::post_json(url, accept), endpoint)
}

fn with_endpoint_headers(
    mut request: ProviderRequest,
    endpoint: &TextCompletionEndpoint,
) -> Result<ProviderRequest, DomainError> {
    for (name, value) in &endpoint.headers {
        request.header(name, value)?;
    }
    Ok(request)
}

fn stream_accept(format: TextCompletionStreamFormat) -> &'static str {
    match format {
        TextCompletionStreamFormat::Sse => EVENT_STREAM,
        TextCompletionStreamFormat::OllamaNdjson => NDJSON,
    }
}

#[async_trait]
impl TextCompletionRepository for HttpTextCompletionRepository {
    async fn get(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
    ) -> Result<Value, DomainError> {
        self.send_json(HttpClientProfile::ProviderMetadata, endpoint, path, None)
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
        self.send_json(HttpClientProfile::Generation, endpoint, path, Some(body))
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
        let response = self
            .send(
                HttpClientProfile::GenerationStream,
                endpoint,
                post_request(endpoint, path, stream_accept(format))?,
                Some(body),
                "Generation request failed",
            )
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

    fn describe_request(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        stream: Option<TextCompletionStreamFormat>,
    ) -> Result<RequestDescriptionDto, DomainError> {
        let request = post_request(endpoint, path, stream.map_or(JSON, stream_accept))?;
        Ok(request.describe(
            RequestTransportDto::Http,
            self.http_clients.product_user_agent(),
            &[],
        ))
    }
}
