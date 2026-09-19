use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde_json::Value;

use crate::dto::vector_dto::{
    VectorRouteRequestDto, VectorRouteResponseDto, VectorRouteResponseKindDto,
};
use crate::errors::ApplicationError;
use tt_domain::ios_policy::{
    AllowlistMode, AllowlistSetting, IosPolicyActivationReport, IosPolicyScope,
};
use tt_domain::models::chat_completion_source::ChatCompletionSource;
use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::vector_repository::{
    LocalEmbeddingModel, LocalEmbeddingRepository, LocalEmbeddingRequest, RemoteEmbeddingProtocol,
    RemoteEmbeddingRepository, RemoteEmbeddingRequest, VectorMatch, VectorMetadata, VectorRecord,
    VectorRepository, VectorScope, VertexEmbeddingAuth,
};

const DEFAULT_TOP_K: usize = 10;
const MAX_TOP_K: usize = 1_000;
const EMBEDDING_BATCH_SIZE: usize = 10;

pub struct VectorService {
    vector_repository: Arc<dyn VectorRepository>,
    remote_embedding_repository: Arc<dyn RemoteEmbeddingRepository>,
    local_embedding_repository: Arc<dyn LocalEmbeddingRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    ios_policy: IosPolicyActivationReport,
}

impl VectorService {
    pub fn new(
        vector_repository: Arc<dyn VectorRepository>,
        remote_embedding_repository: Arc<dyn RemoteEmbeddingRepository>,
        local_embedding_repository: Arc<dyn LocalEmbeddingRepository>,
        secret_repository: Arc<dyn SecretRepository>,
        ios_policy: IosPolicyActivationReport,
    ) -> Self {
        Self {
            vector_repository,
            remote_embedding_repository,
            local_embedding_repository,
            secret_repository,
            ios_policy,
        }
    }

    pub async fn handle_request(
        &self,
        path: &str,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        match path.trim().trim_matches('/').to_ascii_lowercase().as_str() {
            "list" => self.list(request).await,
            "insert" => self.insert(request).await,
            "delete" => self.delete(request).await,
            "query" => self.query(request).await,
            "query-multi" => self.query_multi(request).await,
            "purge" => self.purge(request).await,
            "purge-all" => self.purge_all().await,
            "koboldcpp-embed" => self.koboldcpp_embed(request).await,
            other => Err(ApplicationError::NotFound(format!(
                "Unsupported vector endpoint: {other}"
            ))),
        }
    }

    async fn list(
        &self,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        let collection_id = required_value(&request.collection_id, "collectionId")?;
        let context = SourceContext::from_request(&request)?;
        let hashes = self
            .vector_repository
            .list_hashes(&context.scope(collection_id))
            .await?;
        json_response(hashes)
    }

    async fn insert(
        &self,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        let collection_id = required_value(&request.collection_id, "collectionId")?;
        let context = SourceContext::from_request(&request)?;
        let items = request.items.as_ref().ok_or_else(|| {
            ApplicationError::ValidationError("items must be an array".to_string())
        })?;
        if items.is_empty() {
            return Ok(empty_response());
        }

        let texts = items
            .iter()
            .map(|item| item.text.clone())
            .collect::<Vec<_>>();
        let embeddings = self.embeddings(&context, &request, texts, false).await?;
        let records = request
            .items
            .expect("items were validated")
            .into_iter()
            .zip(embeddings)
            .map(|(item, embedding)| VectorRecord {
                metadata: VectorMetadata {
                    hash: item.hash,
                    text: item.text,
                    index: item.index,
                },
                embedding,
            })
            .collect();

        self.vector_repository
            .upsert(&context.scope(collection_id), records)
            .await?;
        Ok(empty_response())
    }

    async fn delete(
        &self,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        let collection_id = required_value(&request.collection_id, "collectionId")?;
        let context = SourceContext::from_request(&request)?;
        let hashes = request.hashes.as_deref().ok_or_else(|| {
            ApplicationError::ValidationError("hashes must be an array".to_string())
        })?;
        self.vector_repository
            .delete_hashes(&context.scope(collection_id), hashes)
            .await?;
        Ok(empty_response())
    }

    async fn query(
        &self,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        let collection_id = required_value(&request.collection_id, "collectionId")?;
        required_value(&request.search_text, "searchText")?;
        let top_k = top_k(request.top_k)?;
        let threshold = threshold(request.threshold)?;
        let context = SourceContext::from_request(&request)?;
        let embedding = self
            .embeddings(&context, &request, vec![request.search_text.clone()], true)
            .await?
            .into_iter()
            .next()
            .expect("one query embedding was requested");
        let matches = self
            .vector_repository
            .query(&context.scope(collection_id), embedding, top_k)
            .await?;
        json_response(query_result(matches, threshold))
    }

    async fn query_multi(
        &self,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        required_value(&request.search_text, "searchText")?;
        let collection_ids = request
            .collection_ids
            .as_ref()
            .ok_or_else(|| {
                ApplicationError::ValidationError("collectionIds must be an array".to_string())
            })?
            .iter()
            .map(|collection_id| required_value(collection_id, "collectionIds[]"))
            .collect::<Result<Vec<_>, _>>()?;
        if collection_ids.is_empty() {
            return json_response(BTreeMap::<String, VectorQueryResultDto>::new());
        }

        let top_k = top_k(request.top_k)?;
        let threshold = threshold(request.threshold)?;
        let context = SourceContext::from_request(&request)?;
        let embedding = self
            .embeddings(&context, &request, vec![request.search_text.clone()], true)
            .await?
            .into_iter()
            .next()
            .expect("one query embedding was requested");

        let mut matches = Vec::new();
        for collection_id in collection_ids {
            matches.extend(
                self.vector_repository
                    .query(&context.scope(collection_id), embedding.clone(), top_k)
                    .await?
                    .into_iter()
                    .map(|result| (collection_id.to_string(), result)),
            );
        }
        matches.sort_by(|left, right| right.1.score.total_cmp(&left.1.score));
        matches.retain(|(_, result)| result.score >= threshold);
        matches.truncate(top_k);

        let mut grouped = BTreeMap::<String, VectorQueryResultDto>::new();
        for (collection_id, result) in matches {
            let entry = grouped.entry(collection_id).or_default();
            entry.hashes.push(result.metadata.hash);
            entry.metadata.push(result.metadata);
        }
        json_response(grouped)
    }

    async fn purge(
        &self,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        let collection_id = required_value(&request.collection_id, "collectionId")?;
        self.vector_repository
            .purge_collection(collection_id)
            .await?;
        Ok(empty_response())
    }

    async fn purge_all(&self) -> Result<VectorRouteResponseDto, ApplicationError> {
        self.vector_repository.purge_all().await?;
        Ok(empty_response())
    }

    async fn koboldcpp_embed(
        &self,
        request: VectorRouteRequestDto,
    ) -> Result<VectorRouteResponseDto, ApplicationError> {
        let texts = request.texts.clone().ok_or_else(|| {
            ApplicationError::ValidationError("items must be an array".to_string())
        })?;
        let context = SourceContext {
            source: VectorSource::KoboldCpp,
            model: String::new(),
            profile: String::new(),
            local_model: None,
        };
        self.ensure_remote_source_allowed(context.source)?;
        let batch = self
            .remote_embedding_repository
            .embed(RemoteEmbeddingRequest {
                protocol: self.remote_protocol(&context, &request).await?,
                texts,
                is_query: request.is_query,
            })
            .await?;
        json_response(KoboldCppEmbeddingResponseDto {
            embeddings: batch.embeddings,
            model: batch.reported_model.ok_or_else(|| {
                ApplicationError::InternalError(
                    "KoboldCpp embedding response omitted its model".to_string(),
                )
            })?,
        })
    }

    async fn embeddings(
        &self,
        context: &SourceContext,
        request: &VectorRouteRequestDto,
        texts: Vec<String>,
        is_query: bool,
    ) -> Result<Vec<Vec<f32>>, ApplicationError> {
        let expected_count = texts.len();
        let embeddings = match context.source {
            VectorSource::Transformers => {
                self.local_embedding_repository
                    .embed(LocalEmbeddingRequest {
                        model: context
                            .local_model
                            .expect("transformers source has a local embedding model"),
                        texts,
                        is_query,
                    })
                    .await?
            }
            VectorSource::WebLlm => precomputed_embeddings(&request.embeddings, &texts)?,
            VectorSource::KoboldCpp if !request.embeddings.is_empty() => {
                precomputed_embeddings(&request.embeddings, &texts)?
            }
            VectorSource::KoboldCpp => {
                return Err(ApplicationError::ValidationError(
                    "KoboldCpp embeddings and reported model are required".to_string(),
                ));
            }
            _ => {
                self.ensure_remote_source_allowed(context.source)?;
                let protocol = self.remote_protocol(context, request).await?;
                let mut embeddings = Vec::with_capacity(texts.len());
                for batch in texts.chunks(EMBEDDING_BATCH_SIZE) {
                    embeddings.extend(
                        self.remote_embedding_repository
                            .embed(RemoteEmbeddingRequest {
                                protocol: protocol.clone(),
                                texts: batch.to_vec(),
                                is_query,
                            })
                            .await?
                            .embeddings,
                    );
                }
                embeddings
            }
        };

        normalize_embeddings(embeddings, expected_count)
    }

    async fn remote_protocol(
        &self,
        context: &SourceContext,
        request: &VectorRouteRequestDto,
    ) -> Result<RemoteEmbeddingProtocol, ApplicationError> {
        let protocol = match context.source {
            VectorSource::TogetherAi => openai_protocol(
                "Together AI",
                "https://api.together.xyz/v1",
                self.required_secret(SecretKeys::TOGETHERAI, "Together AI")
                    .await?,
                &context.model,
            ),
            VectorSource::Mistral => openai_protocol(
                "Mistral AI",
                "https://api.mistral.ai/v1",
                self.required_secret(SecretKeys::MISTRALAI, "Mistral AI")
                    .await?,
                &context.model,
            ),
            VectorSource::OpenAi => openai_protocol(
                "OpenAI",
                "https://api.openai.com/v1",
                self.required_secret(SecretKeys::OPENAI, "OpenAI").await?,
                &context.model,
            ),
            VectorSource::ElectronHub => openai_protocol(
                "ElectronHub",
                "https://api.electronhub.ai/v1",
                self.required_secret(SecretKeys::ELECTRONHUB, "ElectronHub")
                    .await?,
                &context.model,
            ),
            VectorSource::OpenRouter => RemoteEmbeddingProtocol::OpenAi {
                provider: "OpenRouter".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: self
                    .required_secret(SecretKeys::OPENROUTER, "OpenRouter")
                    .await?,
                model: context.model.clone(),
                omit_model: false,
                headers: HashMap::from([
                    (
                        "HTTP-Referer".to_string(),
                        "https://tauritavern.github.io".to_string(),
                    ),
                    ("X-Title".to_string(), "TauriTavern".to_string()),
                ]),
            },
            VectorSource::Chutes => RemoteEmbeddingProtocol::OpenAi {
                provider: "Chutes".to_string(),
                base_url: format!("https://{}.chutes.ai/v1", context.model),
                api_key: self.required_secret(SecretKeys::CHUTES, "Chutes").await?,
                model: context.model.clone(),
                omit_model: true,
                headers: HashMap::new(),
            },
            VectorSource::NanoGpt => openai_protocol(
                "NanoGPT",
                "https://nano-gpt.com/api/v1",
                self.required_secret(SecretKeys::NANOGPT, "NanoGPT").await?,
                &context.model,
            ),
            VectorSource::SiliconFlow => openai_protocol(
                "SiliconFlow",
                if request.siliconflow_endpoint.trim() == "cn" {
                    "https://api.siliconflow.cn/v1"
                } else {
                    "https://api.siliconflow.com/v1"
                },
                self.required_secret(SecretKeys::SILICONFLOW, "SiliconFlow")
                    .await?,
                &context.model,
            ),
            VectorSource::WorkersAi => {
                let account_id =
                    required_value(&request.workers_ai_account_id, "workers_ai_account_id")?;
                let account_id = utf8_percent_encode(account_id, NON_ALPHANUMERIC);
                openai_protocol(
                    "Cloudflare Workers AI",
                    &format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1"),
                    self.required_secret(SecretKeys::WORKERS_AI, "Cloudflare Workers AI")
                        .await?,
                    &context.model,
                )
            }
            VectorSource::NomicAi => RemoteEmbeddingProtocol::Nomic {
                api_key: self
                    .required_secret(SecretKeys::NOMICAI, "Nomic AI")
                    .await?,
            },
            VectorSource::Cohere => RemoteEmbeddingProtocol::Cohere {
                api_key: self.required_secret(SecretKeys::COHERE, "Cohere").await?,
                model: context.model.clone(),
            },
            VectorSource::Extras => RemoteEmbeddingProtocol::Extras {
                base_url: required_value(&request.extras_url, "extrasUrl")?.to_string(),
                api_key: non_empty(&request.extras_key),
            },
            VectorSource::Palm => RemoteEmbeddingProtocol::GoogleAiStudio {
                api_key: self
                    .required_secret(SecretKeys::MAKERSUITE, "Google AI Studio")
                    .await?,
                model: context.model.clone(),
            },
            VectorSource::VertexAi => RemoteEmbeddingProtocol::VertexAi {
                model: context.model.clone(),
                region: defaulted(&request.vertexai_region, "us-central1"),
                auth: match defaulted(&request.vertexai_auth_mode, "express").as_str() {
                    "express" => VertexEmbeddingAuth::Express {
                        api_key: self
                            .required_secret(SecretKeys::VERTEXAI, "Google Vertex AI")
                            .await?,
                        project_id: non_empty(&request.vertexai_express_project_id),
                    },
                    "full" => VertexEmbeddingAuth::ServiceAccount {
                        json: self
                            .required_secret(
                                SecretKeys::VERTEXAI_SERVICE_ACCOUNT,
                                "Google Vertex AI service account",
                            )
                            .await?,
                    },
                    other => {
                        return Err(ApplicationError::ValidationError(format!(
                            "Unsupported Vertex AI authentication mode: {other}"
                        )));
                    }
                },
            },
            VectorSource::Ollama => RemoteEmbeddingProtocol::Ollama {
                base_url: required_value(&request.api_url, "apiUrl")?.to_string(),
                model: required_value(&context.model, "model")?.to_string(),
                keep: request.keep,
            },
            VectorSource::LlamaCpp => RemoteEmbeddingProtocol::LlamaCpp {
                base_url: required_value(&request.api_url, "apiUrl")?.to_string(),
                api_key: self.optional_secret(SecretKeys::LLAMACPP).await?,
            },
            VectorSource::Vllm => RemoteEmbeddingProtocol::Vllm {
                base_url: required_value(&request.api_url, "apiUrl")?.to_string(),
                api_key: self.optional_secret(SecretKeys::VLLM).await?,
                model: required_value(&context.model, "model")?.to_string(),
            },
            VectorSource::KoboldCpp => RemoteEmbeddingProtocol::KoboldCpp {
                base_url: required_value(&request.api_url, "apiUrl")?.to_string(),
                api_key: self.optional_secret(SecretKeys::KOBOLDCPP).await?,
            },
            VectorSource::Transformers | VectorSource::WebLlm => {
                return Err(ApplicationError::InternalError(
                    "Local embedding source reached remote transport".to_string(),
                ));
            }
        };
        Ok(protocol)
    }

    fn ensure_remote_source_allowed(&self, source: VectorSource) -> Result<(), ApplicationError> {
        if self.ios_policy.scope != IosPolicyScope::Ios {
            return Ok(());
        }

        let allowlist = &self
            .ios_policy
            .capabilities
            .llm
            .chat_completion_sources
            .allowlist;
        let allowed = match allowlist {
            AllowlistSetting::Mode(AllowlistMode::All) => true,
            AllowlistSetting::List(_) => source.policy_source().is_some_and(|source| {
                self.ios_policy
                    .capabilities
                    .llm
                    .chat_completion_sources
                    .allows_source(source)
            }),
        };
        if !allowed {
            return Err(ApplicationError::PermissionDenied(format!(
                "iOS policy disabled vector source: {}",
                source.key()
            )));
        }

        if source.uses_custom_endpoint() && !self.ios_policy.capabilities.llm.endpoint_overrides {
            return Err(ApplicationError::PermissionDenied(
                "iOS policy disabled capability: llm.endpoint_overrides".to_string(),
            ));
        }
        Ok(())
    }

    async fn required_secret(&self, key: &str, provider: &str) -> Result<String, ApplicationError> {
        self.optional_secret(key).await?.ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "{provider} API key is missing. Please configure {key}."
            ))
        })
    }

    async fn optional_secret(&self, key: &str) -> Result<Option<String>, ApplicationError> {
        Ok(self
            .secret_repository
            .read_secret(key, None)
            .await?
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VectorSource {
    Transformers,
    Mistral,
    OpenAi,
    Extras,
    Palm,
    TogetherAi,
    NomicAi,
    Cohere,
    Ollama,
    LlamaCpp,
    Vllm,
    WebLlm,
    KoboldCpp,
    VertexAi,
    ElectronHub,
    OpenRouter,
    Chutes,
    NanoGpt,
    SiliconFlow,
    WorkersAi,
}

impl VectorSource {
    fn parse(value: &str) -> Result<Self, ApplicationError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "transformers" => Ok(Self::Transformers),
            "mistral" => Ok(Self::Mistral),
            "openai" => Ok(Self::OpenAi),
            "extras" => Ok(Self::Extras),
            "palm" => Ok(Self::Palm),
            "togetherai" => Ok(Self::TogetherAi),
            "nomicai" => Ok(Self::NomicAi),
            "cohere" => Ok(Self::Cohere),
            "ollama" => Ok(Self::Ollama),
            "llamacpp" => Ok(Self::LlamaCpp),
            "vllm" => Ok(Self::Vllm),
            "webllm" => Ok(Self::WebLlm),
            "koboldcpp" => Ok(Self::KoboldCpp),
            "vertexai" => Ok(Self::VertexAi),
            "electronhub" => Ok(Self::ElectronHub),
            "openrouter" => Ok(Self::OpenRouter),
            "chutes" => Ok(Self::Chutes),
            "nanogpt" => Ok(Self::NanoGpt),
            "siliconflow" => Ok(Self::SiliconFlow),
            "workers_ai" => Ok(Self::WorkersAi),
            other => Err(ApplicationError::ValidationError(format!(
                "Unknown vector source: {other}"
            ))),
        }
    }

    const fn key(self) -> &'static str {
        match self {
            Self::Transformers => "transformers",
            Self::Mistral => "mistral",
            Self::OpenAi => "openai",
            Self::Extras => "extras",
            Self::Palm => "palm",
            Self::TogetherAi => "togetherai",
            Self::NomicAi => "nomicai",
            Self::Cohere => "cohere",
            Self::Ollama => "ollama",
            Self::LlamaCpp => "llamacpp",
            Self::Vllm => "vllm",
            Self::WebLlm => "webllm",
            Self::KoboldCpp => "koboldcpp",
            Self::VertexAi => "vertexai",
            Self::ElectronHub => "electronhub",
            Self::OpenRouter => "openrouter",
            Self::Chutes => "chutes",
            Self::NanoGpt => "nanogpt",
            Self::SiliconFlow => "siliconflow",
            Self::WorkersAi => "workers_ai",
        }
    }

    const fn policy_source(self) -> Option<ChatCompletionSource> {
        match self {
            Self::OpenAi => Some(ChatCompletionSource::OpenAi),
            Self::Palm => Some(ChatCompletionSource::Makersuite),
            Self::VertexAi => Some(ChatCompletionSource::VertexAi),
            Self::Cohere => Some(ChatCompletionSource::Cohere),
            Self::OpenRouter => Some(ChatCompletionSource::OpenRouter),
            Self::Chutes => Some(ChatCompletionSource::Chutes),
            Self::NanoGpt => Some(ChatCompletionSource::NanoGpt),
            Self::SiliconFlow => Some(ChatCompletionSource::SiliconFlow),
            Self::WorkersAi => Some(ChatCompletionSource::WorkersAi),
            Self::Extras | Self::Ollama | Self::LlamaCpp | Self::Vllm | Self::KoboldCpp => {
                Some(ChatCompletionSource::Custom)
            }
            Self::Transformers
            | Self::Mistral
            | Self::TogetherAi
            | Self::NomicAi
            | Self::WebLlm
            | Self::ElectronHub => None,
        }
    }

    const fn uses_custom_endpoint(self) -> bool {
        matches!(
            self,
            Self::Extras | Self::Ollama | Self::LlamaCpp | Self::Vllm | Self::KoboldCpp
        )
    }
}

struct SourceContext {
    source: VectorSource,
    model: String,
    profile: String,
    local_model: Option<LocalEmbeddingModel>,
}

impl SourceContext {
    fn from_request(request: &VectorRouteRequestDto) -> Result<Self, ApplicationError> {
        let source = VectorSource::parse(&request.source)?;
        let local_model = if source == VectorSource::Transformers {
            let requested = request.model.trim();
            Some(if requested.is_empty() {
                LocalEmbeddingModel::default()
            } else {
                LocalEmbeddingModel::from_id(requested).ok_or_else(|| {
                    let supported = LocalEmbeddingModel::SUPPORTED
                        .map(LocalEmbeddingModel::id)
                        .join(", ");
                    ApplicationError::ValidationError(format!(
                        "Unsupported local embedding model: {requested}. Supported models: {supported}"
                    ))
                })?
            })
        } else {
            None
        };
        let model = local_model
            .map(|model| model.id().to_string())
            .unwrap_or_else(|| source_model(source, &request.model));
        if source == VectorSource::KoboldCpp {
            required_value(&model, "model")?;
        }
        let endpoint = match source {
            VectorSource::Extras => request.extras_url.trim().to_string(),
            VectorSource::Ollama
            | VectorSource::LlamaCpp
            | VectorSource::Vllm
            | VectorSource::KoboldCpp => request.api_url.trim().to_string(),
            VectorSource::SiliconFlow => {
                if request.siliconflow_endpoint.trim() == "cn" {
                    "https://api.siliconflow.cn/v1".to_string()
                } else {
                    "https://api.siliconflow.com/v1".to_string()
                }
            }
            VectorSource::WorkersAi => request.workers_ai_account_id.trim().to_string(),
            VectorSource::VertexAi => format!(
                "{}|{}",
                defaulted(&request.vertexai_region, "us-central1").to_ascii_lowercase(),
                request.vertexai_express_project_id.trim()
            ),
            _ => String::new(),
        };
        let embedding_profile = local_model
            .map(LocalEmbeddingModel::profile)
            .unwrap_or(&model);
        let profile = format!(
            "model={embedding_profile}\nendpoint={}\nauth={}",
            endpoint.trim_end_matches('/'),
            if source == VectorSource::VertexAi {
                defaulted(&request.vertexai_auth_mode, "express")
            } else {
                String::new()
            }
        );
        Ok(Self {
            source,
            model,
            profile,
            local_model,
        })
    }

    fn scope(&self, collection_id: &str) -> VectorScope {
        VectorScope {
            collection_id: collection_id.to_string(),
            source: self.source.key().to_string(),
            profile: self.profile.clone(),
        }
    }
}

#[derive(Default, Serialize)]
struct VectorQueryResultDto {
    hashes: Vec<i64>,
    metadata: Vec<VectorMetadata>,
}

#[derive(Serialize)]
struct KoboldCppEmbeddingResponseDto {
    embeddings: Vec<Vec<f32>>,
    model: String,
}

fn query_result(matches: Vec<VectorMatch>, threshold: f32) -> VectorQueryResultDto {
    let mut result = VectorQueryResultDto::default();
    for item in matches.into_iter().filter(|item| item.score >= threshold) {
        result.hashes.push(item.metadata.hash);
        result.metadata.push(item.metadata);
    }
    result
}

fn openai_protocol(
    provider: &str,
    base_url: &str,
    api_key: String,
    model: &str,
) -> RemoteEmbeddingProtocol {
    RemoteEmbeddingProtocol::OpenAi {
        provider: provider.to_string(),
        base_url: base_url.to_string(),
        api_key,
        model: model.to_string(),
        omit_model: false,
        headers: HashMap::new(),
    }
}

fn source_model(source: VectorSource, requested: &str) -> String {
    match source {
        VectorSource::Mistral => return "mistral-embed".to_string(),
        VectorSource::NomicAi => return "nomic-embed-text-v1.5".to_string(),
        _ => {}
    }
    let requested = requested.trim();
    if !requested.is_empty() {
        return requested.to_string();
    }
    match source {
        VectorSource::OpenAi => "text-embedding-ada-002",
        VectorSource::Palm | VectorSource::VertexAi => "text-embedding-005",
        VectorSource::TogetherAi => "togethercomputer/m2-bert-80M-32k-retrieval",
        VectorSource::Cohere => "embed-english-v3.0",
        VectorSource::Ollama => "mxbai-embed-large",
        VectorSource::ElectronHub => "text-embedding-3-small",
        VectorSource::OpenRouter => "openai/text-embedding-3-large",
        VectorSource::Chutes => "chutes-qwen-qwen3-embedding-8b",
        VectorSource::NanoGpt => "text-embedding-3-small",
        VectorSource::SiliconFlow => "Qwen/Qwen3-Embedding-0.6B",
        VectorSource::WorkersAi => "@cf/baai/bge-m3",
        VectorSource::Extras
        | VectorSource::KoboldCpp
        | VectorSource::Transformers
        | VectorSource::Mistral
        | VectorSource::NomicAi
        | VectorSource::LlamaCpp
        | VectorSource::Vllm
        | VectorSource::WebLlm => "",
    }
    .to_string()
}

fn normalize_embeddings(
    mut embeddings: Vec<Vec<f32>>,
    expected_count: usize,
) -> Result<Vec<Vec<f32>>, ApplicationError> {
    if embeddings.len() != expected_count {
        return Err(ApplicationError::InternalError(format!(
            "Embedding provider returned {} vectors for {expected_count} inputs",
            embeddings.len()
        )));
    }

    let mut dimension = None;
    for embedding in &mut embeddings {
        if embedding.is_empty() {
            return Err(ApplicationError::ValidationError(
                "Embedding must not be empty".to_string(),
            ));
        }
        if embedding.iter().any(|value| !value.is_finite()) {
            return Err(ApplicationError::ValidationError(
                "Embedding contains a non-finite value".to_string(),
            ));
        }
        if let Some(dimension) = dimension {
            if embedding.len() != dimension {
                return Err(ApplicationError::ValidationError(
                    "Embedding batch contains mixed dimensions".to_string(),
                ));
            }
        } else {
            dimension = Some(embedding.len());
        }

        let norm = embedding
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            .sqrt();
        if norm <= f64::EPSILON {
            return Err(ApplicationError::ValidationError(
                "Embedding must not be a zero vector".to_string(),
            ));
        }
        for value in embedding {
            *value = (f64::from(*value) / norm) as f32;
        }
    }
    Ok(embeddings)
}

fn precomputed_embeddings(
    embeddings: &HashMap<String, Vec<f32>>,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, ApplicationError> {
    texts
        .iter()
        .map(|text| {
            embeddings.get(text).cloned().ok_or_else(|| {
                ApplicationError::ValidationError(
                    "Precomputed embedding is missing for a requested text".to_string(),
                )
            })
        })
        .collect()
}

fn required_value<'a>(value: &'a str, field: &str) -> Result<&'a str, ApplicationError> {
    if value.trim().is_empty() {
        Err(ApplicationError::ValidationError(format!(
            "{field} is required"
        )))
    } else {
        Ok(value)
    }
}

fn top_k(value: Option<i64>) -> Result<usize, ApplicationError> {
    let value = match value {
        None | Some(0) => DEFAULT_TOP_K,
        Some(value) if value > 0 && value <= MAX_TOP_K as i64 => value as usize,
        Some(value) if value > MAX_TOP_K as i64 => {
            return Err(ApplicationError::ValidationError(format!(
                "topK must not exceed {MAX_TOP_K}"
            )));
        }
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "topK must be positive".to_string(),
            ));
        }
    };
    Ok(value)
}

fn threshold(value: Option<f32>) -> Result<f32, ApplicationError> {
    let value = value.unwrap_or(0.0);
    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
        return Err(ApplicationError::ValidationError(
            "threshold must be between -1 and 1".to_string(),
        ));
    }
    Ok(value)
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn defaulted(value: &str, default: &str) -> String {
    non_empty(value).unwrap_or_else(|| default.to_string())
}

fn json_response(body: impl Serialize) -> Result<VectorRouteResponseDto, ApplicationError> {
    Ok(VectorRouteResponseDto {
        status: 200,
        kind: VectorRouteResponseKindDto::Json,
        body: serde_json::to_value(body).map_err(|error| {
            ApplicationError::InternalError(format!("Failed to serialize vector response: {error}"))
        })?,
    })
}

fn empty_response() -> VectorRouteResponseDto {
    VectorRouteResponseDto {
        status: 200,
        kind: VectorRouteResponseKindDto::Empty,
        body: Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_rejects_zero_vectors_and_preserves_cosine_direction() {
        assert!(normalize_embeddings(vec![vec![0.0, 0.0]], 1).is_err());
        let normalized = normalize_embeddings(vec![vec![3.0, 4.0]], 1).unwrap();
        assert!((normalized[0][0] - 0.6).abs() < 1e-6);
        assert!((normalized[0][1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn query_threshold_filters_hashes_and_metadata_together() {
        let result = query_result(
            vec![
                VectorMatch {
                    metadata: VectorMetadata {
                        hash: 1,
                        text: "keep".to_string(),
                        index: 0,
                    },
                    score: 0.8,
                },
                VectorMatch {
                    metadata: VectorMetadata {
                        hash: 2,
                        text: "drop".to_string(),
                        index: 1,
                    },
                    score: 0.2,
                },
            ],
            0.5,
        );
        assert_eq!(result.hashes, vec![1]);
        assert_eq!(result.metadata.len(), 1);
    }

    #[test]
    fn endpoint_identity_separates_local_servers_for_the_same_model() {
        let first = SourceContext::from_request(&VectorRouteRequestDto {
            source: "ollama".to_string(),
            model: "embed".to_string(),
            api_url: "http://one.test/".to_string(),
            ..Default::default()
        })
        .unwrap();
        let second = SourceContext::from_request(&VectorRouteRequestDto {
            source: "ollama".to_string(),
            model: "embed".to_string(),
            api_url: "http://two.test".to_string(),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(first.profile, second.profile);
    }

    #[test]
    fn local_model_selection_defaults_to_qwen_and_rejects_unknown_models() {
        let defaulted = SourceContext::from_request(&VectorRouteRequestDto {
            source: "transformers".to_string(),
            ..Default::default()
        })
        .unwrap();
        let qwen = SourceContext::from_request(&VectorRouteRequestDto {
            source: "transformers".to_string(),
            model: LocalEmbeddingModel::Qwen3Embedding06B.id().to_string(),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(
            defaulted.local_model,
            Some(LocalEmbeddingModel::Qwen3Embedding06B)
        );
        assert_eq!(
            qwen.local_model,
            Some(LocalEmbeddingModel::Qwen3Embedding06B)
        );
        assert_eq!(defaulted.profile, qwen.profile);

        let error = SourceContext::from_request(&VectorRouteRequestDto {
            source: "transformers".to_string(),
            model: "unknown/model".to_string(),
            ..Default::default()
        })
        .err()
        .expect("unknown local model must fail");
        assert!(
            error
                .to_string()
                .contains("Unsupported local embedding model")
        );
    }

    #[test]
    fn koboldcpp_scope_requires_the_server_reported_model() {
        let error = SourceContext::from_request(&VectorRouteRequestDto {
            source: "koboldcpp".to_string(),
            api_url: "http://localhost:5001".to_string(),
            ..Default::default()
        })
        .err()
        .expect("KoboldCpp without a reported model must fail");

        assert!(error.to_string().contains("model is required"));
    }
}
