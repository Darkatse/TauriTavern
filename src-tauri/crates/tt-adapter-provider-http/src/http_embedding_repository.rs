use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use serde_json::{Map, Value, json};

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_ports::repositories::vector_repository::{
    RemoteEmbeddingBatch, RemoteEmbeddingProtocol, RemoteEmbeddingRepository,
    RemoteEmbeddingRequest, VertexEmbeddingAuth,
};

use crate::http_chat_completion_repository::vertexai_auth;

const NOMIC_EMBEDDING_URL: &str = "https://api-atlas.nomic.ai/v1/embedding/text";
const GOOGLE_AI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct HttpEmbeddingRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpEmbeddingRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    fn client(&self) -> Result<Client, DomainError> {
        self.http_clients.client(HttpClientProfile::Default)
    }

    async fn send_json(
        &self,
        request: RequestBuilder,
        provider: &str,
    ) -> Result<Value, DomainError> {
        let response = request.send().await.map_err(|error| {
            DomainError::upstream_failure(crate::http_error::reqwest_transport_failure(&error))
        })?;

        if !response.status().is_success() {
            return Err(map_error_response(provider, response).await);
        }

        let endpoint = response.url().clone();
        response.json().await.map_err(|error| {
            if error.is_decode() {
                DomainError::InternalError(format!(
                    "{provider} embedding response is not valid JSON: {error}"
                ))
            } else {
                DomainError::upstream_failure(crate::http_error::reqwest_body_failure(
                    &error,
                    Some(&endpoint),
                ))
            }
        })
    }

    async fn embed_vertex(
        &self,
        model: String,
        region: String,
        auth: VertexEmbeddingAuth,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DomainError> {
        let region = normalized_region(&region);
        let host = vertex_host(&region);
        let instances = texts
            .into_iter()
            .map(|content| json!({ "content": content }))
            .collect::<Vec<_>>();

        let request = match auth {
            VertexEmbeddingAuth::Express {
                api_key,
                project_id,
            } => {
                let path = match project_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    Some(project_id) => format!(
                        "/v1/projects/{project_id}/locations/{region}/publishers/google/models/{model}:predict"
                    ),
                    None => format!("/v1/publishers/google/models/{model}:predict"),
                };
                let url = Url::parse(&format!("{host}{path}")).map_err(invalid_url("Vertex AI"))?;
                self.client()?
                    .post(url)
                    .header("x-goog-api-key", api_key)
                    .json(&json!({ "instances": instances }))
            }
            VertexEmbeddingAuth::ServiceAccount { json: credentials } => {
                let project_id = service_account_project_id(&credentials)?;
                let token = vertexai_auth::get_service_account_access_token(
                    &self.http_clients,
                    &credentials,
                )
                .await?;
                let url = Url::parse(&format!(
                    "{host}/v1/projects/{project_id}/locations/{region}/publishers/google/models/{model}:predict"
                ))
                .map_err(invalid_url("Vertex AI"))?;
                self.client()?
                    .post(url)
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .json(&json!({ "instances": instances }))
            }
        };

        let body = self.send_json(request, "Google Vertex AI").await?;
        let predictions = required_array(&body, "predictions", "Google Vertex AI")?;
        predictions
            .iter()
            .map(|prediction| {
                parse_embedding(
                    prediction
                        .get("embeddings")
                        .and_then(|value| value.get("values"))
                        .ok_or_else(|| {
                            invalid_response("Google Vertex AI", "missing embeddings.values")
                        })?,
                    "Google Vertex AI",
                )
            })
            .collect()
    }
}

#[async_trait]
impl RemoteEmbeddingRepository for HttpEmbeddingRepository {
    async fn embed(
        &self,
        request: RemoteEmbeddingRequest,
    ) -> Result<RemoteEmbeddingBatch, DomainError> {
        let RemoteEmbeddingRequest {
            protocol,
            texts,
            is_query,
        } = request;
        if texts.is_empty() && !matches!(&protocol, RemoteEmbeddingProtocol::KoboldCpp { .. }) {
            return Ok(RemoteEmbeddingBatch {
                embeddings: Vec::new(),
                reported_model: None,
            });
        }
        let expected_count = texts.len();
        let mut reported_model = None;

        let embeddings = match protocol {
            RemoteEmbeddingProtocol::OpenAi {
                provider,
                base_url,
                api_key,
                model,
                omit_model,
                headers,
            } => {
                let url = joined_url(&base_url, "embeddings")?;
                let mut body = json!({ "input": texts, "model": model });
                if omit_model {
                    body["model"] = Value::Null;
                }

                let mut request = self
                    .client()?
                    .post(url)
                    .header(AUTHORIZATION, format!("Bearer {api_key}"))
                    .json(&body);
                for (name, value) in headers {
                    request = request.header(name, value);
                }
                let body = self.send_json(request, &provider).await?;
                parse_indexed_embeddings(&body, "data", &provider)
            }
            RemoteEmbeddingProtocol::Cohere { api_key, model } => {
                let request = self
                    .client()?
                    .post("https://api.cohere.ai/v2/embed")
                    .header(AUTHORIZATION, format!("Bearer {api_key}"))
                    .json(&json!({
                        "texts": texts,
                        "model": model,
                        "embedding_types": ["float"],
                        "input_type": if is_query { "search_query" } else { "search_document" },
                        "truncate": "END",
                    }));
                let body = self.send_json(request, "Cohere").await?;
                parse_embedding_array(
                    body.get("embeddings")
                        .and_then(|value| value.get("float"))
                        .ok_or_else(|| invalid_response("Cohere", "missing embeddings.float"))?,
                    "Cohere",
                )
            }
            RemoteEmbeddingProtocol::Nomic { api_key } => {
                let request = self
                    .client()?
                    .post(NOMIC_EMBEDDING_URL)
                    .header(AUTHORIZATION, format!("Bearer {api_key}"))
                    .json(&json!({
                        "texts": texts,
                        "model": "nomic-embed-text-v1.5",
                    }));
                let body = self.send_json(request, "Nomic AI").await?;
                parse_embedding_array(
                    body.get("embeddings")
                        .ok_or_else(|| invalid_response("Nomic AI", "missing embeddings"))?,
                    "Nomic AI",
                )
            }
            RemoteEmbeddingProtocol::Extras { base_url, api_key } => {
                let url = root_path_url(&base_url, "/api/embeddings/compute", "Extras")?;
                let mut request = self.client()?.post(url).json(&json!({ "text": texts }));
                if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
                    request = request.header(AUTHORIZATION, format!("Bearer {api_key}"));
                }
                let body = self.send_json(request, "Extras").await?;
                parse_embedding_array(
                    body.get("embedding")
                        .ok_or_else(|| invalid_response("Extras", "missing embedding"))?,
                    "Extras",
                )
            }
            RemoteEmbeddingProtocol::GoogleAiStudio { api_key, model } => {
                let url = Url::parse(&format!(
                    "{GOOGLE_AI_BASE}/models/{model}:batchEmbedContents"
                ))
                .map_err(invalid_url("Google AI Studio"))?;
                let requests = texts
                    .into_iter()
                    .map(|text| {
                        json!({
                            "model": format!("models/{model}"),
                            "content": { "parts": [{ "text": text }] },
                        })
                    })
                    .collect::<Vec<_>>();
                let request = self
                    .client()?
                    .post(url)
                    .header("x-goog-api-key", api_key)
                    .json(&json!({ "requests": requests }));
                let body = self.send_json(request, "Google AI Studio").await?;
                required_array(&body, "embeddings", "Google AI Studio")?
                    .iter()
                    .map(|entry| {
                        parse_embedding(
                            entry.get("values").ok_or_else(|| {
                                invalid_response("Google AI Studio", "missing embeddings.values")
                            })?,
                            "Google AI Studio",
                        )
                    })
                    .collect()
            }
            RemoteEmbeddingProtocol::VertexAi {
                model,
                region,
                auth,
            } => self.embed_vertex(model, region, auth, texts).await,
            RemoteEmbeddingProtocol::Ollama {
                base_url,
                model,
                keep,
            } => {
                let url = root_path_url(&base_url, "/api/embed", "Ollama")?;
                let mut body = Map::from_iter([
                    ("input".to_string(), json!(texts)),
                    ("model".to_string(), json!(model)),
                    ("truncate".to_string(), Value::Bool(true)),
                ]);
                if keep {
                    body.insert("keep_alive".to_string(), json!(-1));
                }
                let request = self.client()?.post(url).json(&body);
                let body = self.send_json(request, "Ollama").await?;
                parse_embedding_array(
                    body.get("embeddings")
                        .ok_or_else(|| invalid_response("Ollama", "missing embeddings"))?,
                    "Ollama",
                )
            }
            RemoteEmbeddingProtocol::LlamaCpp { base_url, api_key } => {
                let url = v1_embeddings_url(&base_url, "Llama.cpp")?;
                let request = optional_bearer(
                    self.client()?.post(url).json(&json!({ "input": texts })),
                    api_key,
                );
                let body = self.send_json(request, "Llama.cpp").await?;
                parse_indexed_embeddings(&body, "data", "Llama.cpp")
            }
            RemoteEmbeddingProtocol::Vllm {
                base_url,
                api_key,
                model,
            } => {
                let url = v1_embeddings_url(&base_url, "vLLM")?;
                let request = optional_bearer(
                    self.client()?
                        .post(url)
                        .json(&json!({ "input": texts, "model": model })),
                    api_key,
                );
                let body = self.send_json(request, "vLLM").await?;
                parse_indexed_embeddings(&body, "data", "vLLM")
            }
            RemoteEmbeddingProtocol::KoboldCpp { base_url, api_key } => {
                let url = root_path_url(&base_url, "/api/extra/embeddings", "KoboldCpp")?;
                let request = optional_bearer(
                    self.client()?
                        .post(url)
                        .header(CONTENT_TYPE, "application/json")
                        .json(&json!({ "input": texts })),
                    api_key,
                );
                let body = self.send_json(request, "KoboldCpp").await?;
                reported_model = Some(parse_reported_model(&body, "KoboldCpp")?);
                parse_kobold_embeddings(&body)
            }
        }?;
        validate_embedding_count(&embeddings, expected_count)?;
        Ok(RemoteEmbeddingBatch {
            embeddings,
            reported_model,
        })
    }
}

fn joined_url(base_url: &str, path: &str) -> Result<Url, DomainError> {
    Url::parse(&format!(
        "{}/{}",
        base_url.trim().trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
    .map_err(invalid_url("embedding provider"))
}

fn root_path_url(base_url: &str, path: &str, provider: &'static str) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url.trim()).map_err(invalid_url(provider))?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn v1_embeddings_url(base_url: &str, provider: &'static str) -> Result<Url, DomainError> {
    let base = base_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    Url::parse(&format!("{base}/v1/embeddings")).map_err(invalid_url(provider))
}

fn optional_bearer(request: RequestBuilder, api_key: Option<String>) -> RequestBuilder {
    match api_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(api_key) => request.header(AUTHORIZATION, format!("Bearer {api_key}")),
        None => request,
    }
}

fn parse_indexed_embeddings(
    body: &Value,
    field: &str,
    provider: &str,
) -> Result<Vec<Vec<f32>>, DomainError> {
    let entries = required_array(body, field, provider)?;
    let mut embeddings = entries
        .iter()
        .enumerate()
        .map(|(position, entry)| {
            let index = parse_embedding_index(entry, position, provider)?;
            let embedding = parse_embedding(
                entry
                    .get("embedding")
                    .ok_or_else(|| invalid_response(provider, "missing embedding"))?,
                provider,
            )?;
            Ok((index, embedding))
        })
        .collect::<Result<Vec<_>, DomainError>>()?;
    embeddings.sort_by_key(|(index, _)| *index);
    validate_embedding_indices(&embeddings, provider)?;
    Ok(embeddings
        .into_iter()
        .map(|(_, embedding)| embedding)
        .collect())
}

fn parse_kobold_embeddings(body: &Value) -> Result<Vec<Vec<f32>>, DomainError> {
    let entries = required_array(body, "data", "KoboldCpp")?;
    let mut normalized = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        let entry = entry
            .as_array()
            .and_then(|entries| entries.first())
            .unwrap_or(entry);
        let index = parse_embedding_index(entry, position, "KoboldCpp")?;
        let embedding = parse_embedding(
            entry
                .get("embedding")
                .ok_or_else(|| invalid_response("KoboldCpp", "missing embedding"))?,
            "KoboldCpp",
        )?;
        normalized.push((index, embedding));
    }
    normalized.sort_by_key(|(index, _)| *index);
    validate_embedding_indices(&normalized, "KoboldCpp")?;
    Ok(normalized
        .into_iter()
        .map(|(_, embedding)| embedding)
        .collect())
}

fn parse_reported_model(body: &Value, provider: &str) -> Result<String, DomainError> {
    body.get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .ok_or_else(|| invalid_response(provider, "missing model"))
}

fn parse_embedding_index(
    entry: &Value,
    position: usize,
    provider: &str,
) -> Result<u64, DomainError> {
    match entry.get("index") {
        Some(index) => index.as_u64().ok_or_else(|| {
            invalid_response(provider, "embedding index is not an unsigned integer")
        }),
        None => Ok(position as u64),
    }
}

fn validate_embedding_indices(
    embeddings: &[(u64, Vec<f32>)],
    provider: &str,
) -> Result<(), DomainError> {
    if embeddings
        .iter()
        .enumerate()
        .any(|(expected, (actual, _))| *actual != expected as u64)
    {
        return Err(invalid_response(
            provider,
            "embedding indices are missing or duplicated",
        ));
    }
    Ok(())
}

fn validate_embedding_count(
    embeddings: &[Vec<f32>],
    expected_count: usize,
) -> Result<(), DomainError> {
    if embeddings.len() != expected_count {
        return Err(invalid_response(
            "Embedding provider",
            &format!(
                "returned {} vectors for {expected_count} inputs",
                embeddings.len()
            ),
        ));
    }
    Ok(())
}

fn parse_embedding_array(value: &Value, provider: &str) -> Result<Vec<Vec<f32>>, DomainError> {
    value
        .as_array()
        .ok_or_else(|| invalid_response(provider, "embeddings is not an array"))?
        .iter()
        .map(|embedding| parse_embedding(embedding, provider))
        .collect()
}

fn parse_embedding(value: &Value, provider: &str) -> Result<Vec<f32>, DomainError> {
    let values = value
        .as_array()
        .ok_or_else(|| invalid_response(provider, "embedding is not an array"))?;
    if values.is_empty() {
        return Err(invalid_response(provider, "embedding is empty"));
    }

    values
        .iter()
        .map(|value| {
            let value = value
                .as_f64()
                .ok_or_else(|| invalid_response(provider, "embedding contains a non-number"))?;
            let value = value as f32;
            if !value.is_finite() {
                return Err(invalid_response(
                    provider,
                    "embedding contains a non-finite number",
                ));
            }
            Ok(value)
        })
        .collect()
}

fn required_array<'a>(
    body: &'a Value,
    field: &str,
    provider: &str,
) -> Result<&'a Vec<Value>, DomainError> {
    body.get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response(provider, &format!("missing {field} array")))
}

fn service_account_project_id(credentials: &str) -> Result<String, DomainError> {
    serde_json::from_str::<Value>(credentials)
        .map_err(|error| {
            DomainError::InvalidData(format!(
                "Vertex AI service account JSON parse failed: {error}"
            ))
        })?
        .get("project_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            DomainError::InvalidData(
                "Vertex AI service account JSON is missing project_id".to_string(),
            )
        })
}

fn normalized_region(region: &str) -> String {
    let region = region.trim().to_ascii_lowercase();
    if region.is_empty() {
        "us-central1".to_string()
    } else {
        region
    }
}

fn vertex_host(region: &str) -> String {
    match region {
        "global" => "https://aiplatform.googleapis.com".to_string(),
        "us" | "eu" => format!("https://aiplatform.{region}.rep.googleapis.com"),
        _ => format!("https://{region}-aiplatform.googleapis.com"),
    }
}

fn invalid_url(provider: &'static str) -> impl FnOnce(url::ParseError) -> DomainError {
    move |error| DomainError::InvalidData(format!("Invalid {provider} embedding URL: {error}"))
}

fn invalid_response(provider: &str, detail: &str) -> DomainError {
    DomainError::InternalError(format!(
        "{provider} embedding response is invalid: {detail}"
    ))
}

async fn map_error_response(provider: &str, response: reqwest::Response) -> DomainError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = response_error_message(&body);
    match status {
        StatusCode::BAD_REQUEST => DomainError::InvalidData(message),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            DomainError::AuthenticationError(message)
        }
        StatusCode::TOO_MANY_REQUESTS => DomainError::rate_limited(format!(
            "{provider} embedding request was rate limited: {message}"
        )),
        status if status.is_server_error() => DomainError::transient(format!(
            "{provider} embedding request failed with status {}: {message}",
            status.as_u16()
        )),
        _ => DomainError::InternalError(format!(
            "{provider} embedding request failed with status {}: {message}",
            status.as_u16()
        )),
    }
}

fn response_error_message(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    parsed
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .or_else(|| value.get("detail"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let body = body.trim();
            (!body.is_empty()).then(|| body.chars().take(1_000).collect())
        })
        .unwrap_or_else(|| "Upstream embedding request failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_embeddings_are_sorted_by_index() {
        let body = json!({
            "data": [
                { "index": 1, "embedding": [0.0, 1.0] },
                { "index": 0, "embedding": [1.0, 0.0] }
            ]
        });

        assert_eq!(
            parse_indexed_embeddings(&body, "data", "test").unwrap(),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
    }

    #[test]
    fn malformed_explicit_embedding_index_is_rejected() {
        let body = json!({ "data": [{ "index": "0", "embedding": [1.0] }] });
        assert!(parse_indexed_embeddings(&body, "data", "test").is_err());
    }

    #[test]
    fn malformed_embeddings_fail_instead_of_becoming_zero_vectors() {
        let body = json!({ "embeddings": [[0.0, "bad"]] });
        assert!(parse_embedding_array(&body["embeddings"], "test").is_err());
    }

    #[test]
    fn local_v1_urls_keep_exactly_one_v1_segment() {
        assert_eq!(
            v1_embeddings_url("http://localhost:8080/v1", "test")
                .unwrap()
                .as_str(),
            "http://localhost:8080/v1/embeddings"
        );
    }

    #[test]
    fn response_count_must_match_the_requested_batch() {
        assert!(validate_embedding_count(&[vec![1.0]], 2).is_err());
    }

    #[test]
    fn reported_model_must_be_present_and_non_empty() {
        assert_eq!(
            parse_reported_model(&json!({ "model": " embed.gguf " }), "test").unwrap(),
            "embed.gguf"
        );
        assert!(parse_reported_model(&json!({}), "test").is_err());
    }
}
