use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::ACCEPT;
use reqwest::{Method, RequestBuilder, StatusCode, Url};
use serde_json::Value;

use tt_adapter_http::{HttpClientPool, HttpClientProfile, upstream};
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::append_endpoint_segments;
use tt_ports::repositories::horde_repository::HordeRepository;

use crate::provider_response;

const API_URL: &str = "https://aihorde.net/api";
const TEXT_MODEL_REFERENCE_URL: &str =
    "https://raw.githubusercontent.com/db0/AI-Horde-text-model-reference/main/db.json";
const PROVIDER_NAME: &str = "AI Horde";
const CLIENT_AGENT_HEADER: &str = "Client-Agent";
const API_KEY_HEADER: &str = "apikey";
const REQUEST_FAILED: &str = "Request failed";

pub struct HttpHordeRepository {
    http_clients: Arc<HttpClientPool>,
    client_agent: String,
    api_url: String,
    text_model_reference_url: String,
}

impl HttpHordeRepository {
    pub fn new(http_clients: Arc<HttpClientPool>, client_agent: impl Into<String>) -> Self {
        Self::with_urls(
            http_clients,
            client_agent,
            API_URL,
            TEXT_MODEL_REFERENCE_URL,
        )
    }

    fn with_urls(
        http_clients: Arc<HttpClientPool>,
        client_agent: impl Into<String>,
        api_url: &str,
        text_model_reference_url: &str,
    ) -> Self {
        Self {
            http_clients,
            client_agent: client_agent.into(),
            api_url: api_url.to_string(),
            text_model_reference_url: text_model_reference_url.to_string(),
        }
    }

    fn api_url(&self, segments: &[&str]) -> Result<Url, DomainError> {
        append_endpoint_segments(&self.api_url, segments)
    }

    fn text_list_url(&self, segments: &[&str]) -> Result<Url, DomainError> {
        let mut url = self.api_url(segments)?;
        url.query_pairs_mut().append_pair("type", "text");
        Ok(url)
    }

    fn request(&self, method: Method, url: Url) -> Result<RequestBuilder, DomainError> {
        Ok(self
            .http_clients
            .client(HttpClientProfile::ProviderMetadata)?
            .request(method, url)
            .header(CLIENT_AGENT_HEADER, &self.client_agent)
            .header(ACCEPT, "application/json"))
    }

    async fn send(&self, request: RequestBuilder) -> Result<reqwest::Response, DomainError> {
        provider_response::send_checked(request, PROVIDER_NAME, REQUEST_FAILED).await
    }

    async fn send_json(
        &self,
        request: RequestBuilder,
        operation: &str,
    ) -> Result<Value, DomainError> {
        let response = self.send(request).await?;
        provider_response::read_upstream_json_body(PROVIDER_NAME, operation, response).await
    }

    fn text_generation_status_url(&self, task_id: &str) -> Result<Url, DomainError> {
        self.api_url(&["v2", "generate", "text", "status", task_id])
    }
}

#[async_trait]
impl HordeRepository for HttpHordeRepository {
    async fn heartbeat(&self) -> Result<(), DomainError> {
        let url = self.api_url(&["v2", "status", "heartbeat"])?;
        self.send(self.request(Method::GET, url)?).await?;
        Ok(())
    }

    async fn text_workers(&self) -> Result<Value, DomainError> {
        let url = self.text_list_url(&["v2", "workers"])?;
        self.send_json(self.request(Method::GET, url)?, "text workers")
            .await
    }

    async fn text_models(&self) -> Result<Value, DomainError> {
        let url = self.text_list_url(&["v2", "status", "models"])?;
        self.send_json(self.request(Method::GET, url)?, "text models")
            .await
    }

    async fn text_model_reference(&self) -> Result<Value, DomainError> {
        let request = self
            .http_clients
            .client(HttpClientProfile::ProviderMetadata)?
            .get(&self.text_model_reference_url)
            .header(ACCEPT, "application/json");
        self.send_json(request, "text model reference").await
    }

    async fn find_user(&self, api_key: &str) -> Result<Value, DomainError> {
        let url = self.api_url(&["v2", "find_user"])?;
        let request = self
            .request(Method::GET, url)?
            .header(API_KEY_HEADER, api_key);
        self.send_json(request, "user").await
    }

    async fn shared_key(&self, api_key: &str) -> Result<Option<Value>, DomainError> {
        // The key is part of the URL, so failures leave the URL out.
        let url = self.api_url(&["v2", "sharedkeys", api_key])?;
        let response = self
            .request(Method::GET, url)?
            .send()
            .await
            .map_err(|error| upstream::request_failure(error.without_url()))?;
        let status = response.status();
        // A personal key is not a shared key; the Horde answers that with 400, 401 or 404.
        if matches!(
            status,
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND
        ) {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(provider_response::map_error_response(
                PROVIDER_NAME,
                response,
                REQUEST_FAILED,
            )
            .await);
        }

        provider_response::read_upstream_json_body_without_url(
            PROVIDER_NAME,
            "shared key",
            response,
        )
        .await
        .map(Some)
    }

    async fn submit_text_generation(
        &self,
        body: &Value,
        api_key: &str,
    ) -> Result<Value, DomainError> {
        let url = self.api_url(&["v2", "generate", "text", "async"])?;
        let request = self
            .request(Method::POST, url)?
            .header(API_KEY_HEADER, api_key)
            .json(body);
        self.send_json(request, "text generation").await
    }

    async fn text_generation_status(&self, task_id: &str) -> Result<Value, DomainError> {
        let url = self.text_generation_status_url(task_id)?;
        self.send_json(self.request(Method::GET, url)?, "text generation status")
            .await
    }

    async fn cancel_text_generation(&self, task_id: &str) -> Result<(), DomainError> {
        let url = self.text_generation_status_url(task_id)?;
        self.send(self.request(Method::DELETE, url)?).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn repository(api_url: &str) -> HttpHordeRepository {
        HttpHordeRepository::with_urls(
            Arc::new(HttpClientPool::new("TauriTavern/test")),
            "TauriTavern:test:contact",
            api_url,
            "http://127.0.0.1:9/unused",
        )
    }

    #[tokio::test]
    async fn requests_identify_the_client_and_horde_errors_keep_their_message() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_url = format!("http://{}/api", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !String::from_utf8_lossy(&request).contains("\"prompt\":\"Hi\"") {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
            }

            let body = br#"{"message":"No available workers","rc":"NoValidWorkers"}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 400 BAD REQUEST\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
            String::from_utf8(request).unwrap().to_ascii_lowercase()
        });

        let error = repository(&api_url)
            .submit_text_generation(&json!({ "prompt": "Hi" }), "user-key")
            .await
            .unwrap_err();
        let wire_request = server.await.unwrap();

        assert!(wire_request.starts_with("post /api/v2/generate/text/async http/1.1\r\n"));
        assert!(wire_request.contains("\r\nclient-agent: tauritavern:test:contact\r\n"));
        assert!(wire_request.contains("\r\napikey: user-key\r\n"));
        assert!(
            matches!(&error, DomainError::InvalidData(message) if message == "No available workers"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_personal_key_is_no_shared_key_and_a_failed_lookup_leaves_the_key_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_url = format!("http://{}/api", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = [0; 1024];
            let _ = stream.read(&mut head).await.unwrap();
            let body = br#"{"message":"No user matching sent Shared Key.","rc":"InvalidAPIKey"}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 401 UNAUTHORIZED\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
        });
        let repository = repository(&api_url);

        assert_eq!(repository.shared_key("personal-key").await.unwrap(), None);
        server.await.unwrap();
        let error = repository.shared_key("secret-key").await.unwrap_err();

        assert!(
            matches!(&error, DomainError::UpstreamFailure(failure) if failure.endpoint.is_none()),
            "{error:?}"
        );
        assert!(!error.to_string().contains("secret-key"));
    }
}
