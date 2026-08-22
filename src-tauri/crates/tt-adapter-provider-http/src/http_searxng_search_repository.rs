use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE};

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_ports::repositories::searxng_search_repository::{
    SearxngSearchRepository, SearxngSearchRequest,
};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub struct HttpSearxngSearchRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpSearxngSearchRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }
}

#[async_trait]
impl SearxngSearchRepository for HttpSearxngSearchRepository {
    async fn search(&self, request: SearxngSearchRequest) -> Result<String, DomainError> {
        let client = self
            .http_clients
            .user_endpoint_client(HttpClientProfile::WebSearch, request.base_url.as_str())?;
        let url = search_url(&request);
        let mut response = client
            .get(url)
            .header(ACCEPT, "text/html,application/xhtml+xml")
            .header(ACCEPT_LANGUAGE, "en-US,en;q=0.5")
            .send()
            .await
            .map_err(|error| {
                DomainError::upstream_failure(crate::http_error::reqwest_transport_failure(&error))
            })?;

        if !response.status().is_success() {
            return Err(DomainError::InternalError(format!(
                "SearXNG search failed with HTTP {}",
                response.status()
            )));
        }

        let endpoint = response.url().clone();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            DomainError::upstream_failure(crate::http_error::reqwest_body_failure(
                &error,
                Some(&endpoint),
            ))
        })? {
            append_body_chunk(&mut body, &chunk)?;
        }

        String::from_utf8(body).map_err(|_| {
            DomainError::InternalError("SearXNG response is not valid UTF-8".to_string())
        })
    }
}

fn search_url(request: &SearxngSearchRequest) -> url::Url {
    let mut url = request.base_url.clone();
    url.set_path("/search");
    let mut query = url.query_pairs_mut();
    query.append_pair("q", &request.query);
    if let Some(preferences) = &request.preferences {
        query.append_pair("preferences", preferences);
    }
    if let Some(categories) = &request.categories {
        query.append_pair("categories", categories);
    }
    drop(query);
    url
}

fn append_body_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), DomainError> {
    if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
        return Err(DomainError::InternalError(format!(
            "SearXNG response exceeds {MAX_RESPONSE_BYTES} bytes"
        )));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tt_ports::user_endpoint_access::UserEndpointGrantRuntime;

    #[tokio::test]
    async fn search_preserves_the_upstream_http_contract() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
            }

            let body = b"<article class=\"result\">Tauri</article>";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
            String::from_utf8(request).unwrap()
        });

        let pool = Arc::new(HttpClientPool::new("TauriTavern/test"));
        let request = SearxngSearchRequest {
            base_url: url::Url::parse(&base_url).unwrap(),
            query: "rust & tauri".to_string(),
            preferences: Some("lang=en".to_string()),
            categories: Some("it,science".to_string()),
        };
        let repository = HttpSearxngSearchRepository::new(pool.clone());

        pool.replace_user_endpoint_grants(&[request.base_url.to_string()]);

        assert_eq!(
            repository.search(request).await.unwrap(),
            "<article class=\"result\">Tauri</article>"
        );
        let wire_request = server.await.unwrap();

        assert!(wire_request.starts_with(
            "GET /search?q=rust+%26+tauri&preferences=lang%3Den&categories=it%2Cscience HTTP/1.1\r\n"
        ));
    }
}
