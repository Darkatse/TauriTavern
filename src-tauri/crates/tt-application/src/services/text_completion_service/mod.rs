//! Text completion: the page applies the prompt template and TauriTavern relays requests to the
//! user's server, following the SillyTavern text completion server routes.

use std::sync::Arc;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use tt_domain::ios_policy::{IosPolicyActivationReport, IosPolicyScope};
use tt_domain::models::endpoint_url::parse_user_http_endpoint;
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::text_completion_repository::{
    TextCompletionEndpoint, TextCompletionRepository, TextCompletionType,
};

use crate::dto::text_completion_dto::{
    TextCompletionEncodeRequestDto, TextCompletionGenerateRequestDto,
    TextCompletionOllamaDownloadRequestDto, TextCompletionPropsRequestDto, TextCompletionStatusDto,
    TextCompletionStatusRequestDto, TextCompletionTabbyDownloadRequestDto, TextCompletionTokensDto,
};
use crate::errors::ApplicationError;
use crate::services::generation_runtime::{
    GenerationRuntime, InterruptCleanup, RegisteredRequest, notifies_completion,
};
use crate::services::hashing::hex_lower;
use crate::services::selected_secret::{read_selected_secret, requested_secret_id};

mod backend;
mod ollama;

const RFC3986_UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The server a request names: its type, and its URL without the `/v1` suffix.
pub struct TextCompletionServer {
    api_type: TextCompletionType,
    base_url: Url,
}

impl TextCompletionServer {
    pub fn endpoint(&self) -> &Url {
        &self.base_url
    }
}

pub struct PreparedTextCompletionProps {
    server: TextCompletionServer,
    model: Option<String>,
}

impl PreparedTextCompletionProps {
    pub fn endpoint(&self) -> &Url {
        self.server.endpoint()
    }
}

pub struct PreparedTextCompletionGeneration {
    server: TextCompletionServer,
    secret_id: Option<String>,
    notify_completion: bool,
    body: Value,
}

impl PreparedTextCompletionGeneration {
    pub fn endpoint(&self) -> &Url {
        self.server.endpoint()
    }
}

pub struct PreparedTextCompletionEncode {
    server: TextCompletionServer,
    path: &'static str,
    body: Value,
}

impl PreparedTextCompletionEncode {
    pub fn endpoint(&self) -> &Url {
        self.server.endpoint()
    }
}

pub struct PreparedTextCompletionDownload {
    server: TextCompletionServer,
    path: &'static str,
    body: Value,
}

impl PreparedTextCompletionDownload {
    pub fn endpoint(&self) -> &Url {
        self.server.endpoint()
    }
}

pub struct TextCompletionService {
    repository: Arc<dyn TextCompletionRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    generation_runtime: Arc<GenerationRuntime>,
    ios_policy: IosPolicyActivationReport,
}

impl TextCompletionService {
    pub fn new(
        repository: Arc<dyn TextCompletionRepository>,
        secret_repository: Arc<dyn SecretRepository>,
        generation_runtime: Arc<GenerationRuntime>,
        ios_policy: IosPolicyActivationReport,
    ) -> Self {
        Self {
            repository,
            secret_repository,
            generation_runtime,
            ios_policy,
        }
    }

    pub fn prepare_status(
        &self,
        dto: TextCompletionStatusRequestDto,
    ) -> Result<TextCompletionServer, ApplicationError> {
        self.resolve_server(parse_type(&dto.api_type)?, &dto.api_server)
    }

    pub fn prepare_props(
        &self,
        dto: TextCompletionPropsRequestDto,
    ) -> Result<PreparedTextCompletionProps, ApplicationError> {
        Ok(PreparedTextCompletionProps {
            server: self.resolve_server(parse_type(&dto.api_type)?, &dto.api_server)?,
            model: dto.model,
        })
    }

    pub fn prepare_generation(
        &self,
        dto: TextCompletionGenerateRequestDto,
    ) -> Result<PreparedTextCompletionGeneration, ApplicationError> {
        let field = |key| string_field(&dto.payload, key);
        let server = self.resolve_server(
            parse_type(field("api_type")?.unwrap_or_default())?,
            field("api_server")?.unwrap_or_default(),
        )?;
        let secret_id = requested_secret_id(field("secret_id")?).map(str::to_owned);
        let notify_completion = notifies_completion(&dto.payload);
        let body = backend::generate_body(server.api_type, dto.payload)?;
        Ok(PreparedTextCompletionGeneration {
            server,
            secret_id,
            notify_completion,
            body,
        })
    }

    pub fn prepare_encode(
        &self,
        dto: TextCompletionEncodeRequestDto,
    ) -> Result<PreparedTextCompletionEncode, ApplicationError> {
        let server = self.resolve_server(parse_type(&dto.api_type)?, &dto.api_server)?;
        let model = dto
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty());
        let (path, body) =
            backend::encode_request(server.api_type, &dto.text, model).ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "{} does not tokenize text",
                    server.api_type.display_name()
                ))
            })?;
        Ok(PreparedTextCompletionEncode { server, path, body })
    }

    pub fn prepare_ollama_download(
        &self,
        dto: TextCompletionOllamaDownloadRequestDto,
    ) -> Result<PreparedTextCompletionDownload, ApplicationError> {
        Ok(PreparedTextCompletionDownload {
            server: self.resolve_server(TextCompletionType::Ollama, &dto.api_server)?,
            path: "/api/pull",
            body: json!({ "model": dto.name.trim(), "stream": false }),
        })
    }

    pub fn prepare_tabby_download(
        &self,
        dto: TextCompletionTabbyDownloadRequestDto,
    ) -> Result<PreparedTextCompletionDownload, ApplicationError> {
        let api_server = string_field(&dto.payload, "api_server")?.unwrap_or_default();
        Ok(PreparedTextCompletionDownload {
            server: self.resolve_server(TextCompletionType::Tabby, api_server)?,
            path: "/v1/download",
            body: Value::Object(backend::without_host_fields(dto.payload)),
        })
    }

    pub async fn get_status(
        &self,
        server: TextCompletionServer,
    ) -> Result<TextCompletionStatusDto, ApplicationError> {
        let endpoint = self.endpoint_with_auth(server, None).await?;
        let api_type = endpoint.api_type;
        let response = self
            .repository
            .get(&endpoint, backend::models_path(api_type))
            .await?;
        let data = backend::models(api_type, response)?;
        let mut result = data
            .first()
            .and_then(|model| non_empty_str(model, "id"))
            .unwrap_or("Valid")
            .to_string();
        let mut supports_tokenization = false;

        match api_type {
            TextCompletionType::Ooba => {
                // Model info is optional; a server without it does not tokenize.
                if let Ok(info) = self
                    .repository
                    .get(&endpoint, "/v1/internal/model/info")
                    .await
                {
                    if let Some(name) = non_empty_str(&info, "model_name") {
                        result = name.to_string();
                    }
                    supports_tokenization = true;
                }
            }
            TextCompletionType::Tabby => {
                // An error means TabbyAPI has no model loaded.
                result = match self.repository.get(&endpoint, "/v1/model").await {
                    Ok(model) => non_empty_str(&model, "id").unwrap_or(&result).to_string(),
                    Err(_) => "None".to_string(),
                };
            }
            TextCompletionType::Vllm
            | TextCompletionType::Aphrodite
            | TextCompletionType::KoboldCpp
            | TextCompletionType::LlamaCpp
            | TextCompletionType::Ollama
            | TextCompletionType::HuggingFace
            | TextCompletionType::Generic => {}
        }

        Ok(TextCompletionStatusDto {
            result,
            data,
            supports_tokenization,
        })
    }

    /// The server's props, with `chat_template_hash` for the page's template table.
    pub async fn get_props(
        &self,
        prepared: PreparedTextCompletionProps,
    ) -> Result<Value, ApplicationError> {
        let endpoint = self.endpoint_with_auth(prepared.server, None).await?;
        let api_type = endpoint.api_type;
        let path = match prepared.model.as_deref().map(str::trim) {
            Some(model) if api_type == TextCompletionType::LlamaCpp && !model.is_empty() => {
                format!(
                    "/props?model={}",
                    utf8_percent_encode(model, RFC3986_UNRESERVED)
                )
            }
            _ => "/props".to_string(),
        };

        let mut props = self.repository.get(&endpoint, &path).await?;
        let Some(template) = props.get("chat_template").and_then(Value::as_str) else {
            return Err(ApplicationError::ValidationError(format!(
                "{} props have no chat_template",
                api_type.display_name()
            )));
        };
        // llama.cpp ends the template with NUL in place of its final newline.
        let hash = if api_type == TextCompletionType::LlamaCpp
            && let Some(stripped) = template.strip_suffix('\0')
        {
            let normalized = format!("{stripped}\n");
            let hash = hex_lower(&Sha256::digest(normalized.as_bytes()));
            props["chat_template"] = Value::String(normalized);
            hash
        } else {
            hex_lower(&Sha256::digest(template.as_bytes()))
        };
        props["chat_template_hash"] = Value::String(hash);
        Ok(props)
    }

    pub async fn encode(
        &self,
        prepared: PreparedTextCompletionEncode,
    ) -> Result<TextCompletionTokensDto, ApplicationError> {
        let endpoint = self.endpoint_with_auth(prepared.server, None).await?;
        let response = self
            .repository
            .post(&endpoint, prepared.path, &prepared.body)
            .await?;
        backend::encoded_tokens(endpoint.api_type, response)
    }

    pub async fn download(
        &self,
        prepared: PreparedTextCompletionDownload,
    ) -> Result<(), ApplicationError> {
        let endpoint = self.endpoint_with_auth(prepared.server, None).await?;
        if endpoint.api_type == TextCompletionType::Tabby {
            let permission = self
                .repository
                .get(&endpoint, "/v1/auth/permission")
                .await?;
            if permission.get("permission").and_then(Value::as_str) != Some("admin") {
                return Err(ApplicationError::PermissionDenied(
                    "TabbyAPI downloads models only with an admin key".to_string(),
                ));
            }
        }

        self.repository
            .download(&endpoint, prepared.path, &prepared.body)
            .await?;
        Ok(())
    }

    pub async fn generate_request(
        &self,
        request: RegisteredRequest<'_>,
        prepared: PreparedTextCompletionGeneration,
    ) -> Result<Value, ApplicationError> {
        let endpoint = self
            .endpoint_with_auth(prepared.server, prepared.secret_id.as_deref())
            .await?;
        let on_interrupt = self.interrupt_cleanup(&endpoint);

        self.generation_runtime
            .run_request(request, prepared.notify_completion, on_interrupt, async {
                Ok(self
                    .repository
                    .generate(
                        &endpoint,
                        backend::generate_path(endpoint.api_type),
                        &prepared.body,
                    )
                    .await?)
            })
            .await
    }

    pub async fn start_stream(
        &self,
        stream_id: String,
        prepared: PreparedTextCompletionGeneration,
    ) -> Result<(), ApplicationError> {
        let endpoint = self
            .endpoint_with_auth(prepared.server, prepared.secret_id.as_deref())
            .await?;
        let on_interrupt = self.interrupt_cleanup(&endpoint);
        let repository = self.repository.clone();
        let body = prepared.body;

        self.generation_runtime
            .start_stream(
                stream_id,
                prepared.notify_completion,
                on_interrupt,
                move |sender, cancel| async move {
                    let api_type = endpoint.api_type;
                    Ok(repository
                        .generate_stream(
                            &endpoint,
                            backend::generate_path(api_type),
                            &body,
                            backend::stream_format(api_type),
                            sender,
                            cancel,
                        )
                        .await?)
                },
            )
            .await
    }

    fn interrupt_cleanup(&self, endpoint: &TextCompletionEndpoint) -> Option<InterruptCleanup> {
        let path = backend::abort_path(endpoint.api_type)?;
        let repository = self.repository.clone();
        let endpoint = endpoint.clone();
        Some(Box::pin(async move {
            if let Err(error) = repository.post(&endpoint, path, &json!({})).await {
                tracing::warn!(
                    api_type = endpoint.api_type.key(),
                    "Failed to stop the interrupted generation: {error}"
                );
            }
        }))
    }

    async fn endpoint_with_auth(
        &self,
        server: TextCompletionServer,
        secret_id: Option<&str>,
    ) -> Result<TextCompletionEndpoint, ApplicationError> {
        let TextCompletionServer { api_type, base_url } = server;
        let headers = match backend::secret_key(api_type) {
            Some(secret_key) => {
                read_selected_secret(self.secret_repository.as_ref(), secret_key, secret_id)
                    .await?
                    .map(|api_key| backend::auth_headers(api_type, &api_key))
                    .unwrap_or_default()
            }
            None => Vec::new(),
        };

        Ok(TextCompletionEndpoint {
            api_type,
            base_url: base_url.to_string(),
            headers,
        })
    }

    /// Every request resolves its server here before consent and any network request, so the iOS
    /// policy is checked here. The URL may end in `/v1`.
    fn resolve_server(
        &self,
        api_type: TextCompletionType,
        api_server: &str,
    ) -> Result<TextCompletionServer, ApplicationError> {
        self.ensure_text_completions_allowed()?;
        if api_server.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "{} server URL is missing",
                api_type.display_name()
            )));
        }

        let mut url = parse_user_http_endpoint(api_server)?;
        if let Some(path) = url.path().strip_suffix("/v1").map(str::to_owned) {
            url.set_path(&path);
        }
        Ok(TextCompletionServer {
            api_type,
            base_url: url,
        })
    }

    fn ensure_text_completions_allowed(&self) -> Result<(), ApplicationError> {
        if self.ios_policy.scope == IosPolicyScope::Ios
            && !self.ios_policy.capabilities.llm.text_completions.enabled
        {
            return Err(ApplicationError::PermissionDenied(
                "iOS policy disabled capability: llm.text_completions.enabled".to_string(),
            ));
        }
        Ok(())
    }
}

fn parse_type(api_type: &str) -> Result<TextCompletionType, ApplicationError> {
    TextCompletionType::parse(api_type).ok_or_else(|| {
        ApplicationError::ValidationError(format!("Unsupported text completion type: {api_type}"))
    })
}

fn string_field<'a>(
    payload: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, ApplicationError> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.as_str())),
        Some(_) => Err(ApplicationError::ValidationError(format!(
            "Text completion request field must be a string: {key}"
        ))),
    }
}

fn non_empty_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}
