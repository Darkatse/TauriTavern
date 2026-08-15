use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use candle_core::{DType, Device};
use candle_nn::VarBuilder;
use fastembed::{
    EmbeddingModel, Qwen3Config, Qwen3Model, Qwen3TextEmbedding, TextEmbedding, TextInitOptions,
};
use hf_hub::api::sync::ApiBuilder;
use tokenizers::{PaddingParams, PaddingStrategy, TruncationParams};

use tt_domain::errors::DomainError;
use tt_ports::repositories::vector_repository::{
    LocalEmbeddingModel, LocalEmbeddingRepository, LocalEmbeddingRequest,
};

const QWEN_MAX_LENGTH: usize = 8_192;
const QWEN_RETRIEVAL_INSTRUCTION: &str =
    "Given a conversation context, retrieve relevant passages and memories";

pub struct FastEmbedLocalEmbeddingRepository {
    cache_dir: PathBuf,
    // ponytail: one model lock bounds memory and inference concurrency; add a worker pool only
    // if measured queue latency justifies loading or scheduling more runtime state.
    model: Arc<Mutex<Option<LoadedEmbeddingModel>>>,
}

enum LoadedEmbeddingModel {
    Onnx {
        model: LocalEmbeddingModel,
        runtime: TextEmbedding,
    },
    Qwen3(Qwen3TextEmbedding),
}

impl LoadedEmbeddingModel {
    fn load(model: LocalEmbeddingModel, cache_dir: &Path) -> Result<Self, fastembed::Error> {
        let (runtime_model, max_length) = match model {
            LocalEmbeddingModel::JinaV2BaseEn => (EmbeddingModel::JinaEmbeddingsV2BaseEN, 512),
            LocalEmbeddingModel::BgeM3 => (EmbeddingModel::BGEM3, 8_192),
            LocalEmbeddingModel::EmbeddingGemma300M => {
                (EmbeddingModel::EmbeddingGemma300MQ4, 2_048)
            }
            LocalEmbeddingModel::Qwen3Embedding06B => {
                return Ok(Self::Qwen3(load_qwen3(cache_dir)?));
            }
        };
        let options = TextInitOptions::new(runtime_model)
            .with_cache_dir(cache_dir.to_path_buf())
            .with_max_length(max_length)
            .with_intra_threads(2)
            .with_show_download_progress(false);
        Ok(Self::Onnx {
            model,
            runtime: TextEmbedding::try_new(options)?,
        })
    }

    const fn model(&self) -> LocalEmbeddingModel {
        match self {
            Self::Onnx { model, .. } => *model,
            Self::Qwen3(_) => LocalEmbeddingModel::Qwen3Embedding06B,
        }
    }

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, fastembed::Error> {
        match self {
            Self::Onnx { runtime, .. } => runtime.embed(texts, Some(10)),
            Self::Qwen3(runtime) => runtime.embed(texts).map_err(fastembed::Error::new),
        }
    }
}

impl FastEmbedLocalEmbeddingRepository {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            model: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl LocalEmbeddingRepository for FastEmbedLocalEmbeddingRepository {
    async fn embed(&self, request: LocalEmbeddingRequest) -> Result<Vec<Vec<f32>>, DomainError> {
        let cache_dir = self.cache_dir.clone();
        let model = self.model.clone();
        tokio::task::spawn_blocking(move || {
            let texts = prepare_texts(request.model, request.texts, request.is_query);
            let mut loaded = model.lock().map_err(|_| {
                DomainError::InternalError("Local embedding model lock was poisoned".to_string())
            })?;

            if loaded.as_ref().map(LoadedEmbeddingModel::model) != Some(request.model) {
                *loaded = None;
                *loaded = Some(
                    LoadedEmbeddingModel::load(request.model, &cache_dir).map_err(|error| {
                        DomainError::transient(format!(
                            "Failed to initialize local embedding model {}: {error}",
                            request.model.id()
                        ))
                    })?,
                );
            }

            loaded
                .as_mut()
                .expect("local embedding model was initialized")
                .embed(&texts)
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Local embedding inference failed for {}: {error}",
                        request.model.id()
                    ))
                })
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Local embedding task failed: {error}"))
        })?
    }
}

fn prepare_texts(model: LocalEmbeddingModel, texts: Vec<String>, is_query: bool) -> Vec<String> {
    texts
        .into_iter()
        .map(|text| match (model, is_query) {
            (LocalEmbeddingModel::Qwen3Embedding06B, true) => {
                format!("Instruct: {QWEN_RETRIEVAL_INSTRUCTION}\nQuery:{text}")
            }
            (LocalEmbeddingModel::EmbeddingGemma300M, true) => {
                format!("task: search result | query: {text}")
            }
            (LocalEmbeddingModel::EmbeddingGemma300M, false) => {
                format!("title: none | text: {text}")
            }
            _ => text,
        })
        .collect()
}

fn load_qwen3(cache_dir: &Path) -> Result<Qwen3TextEmbedding, fastembed::Error> {
    // FastEmbed's convenience loader does not accept a cache path. Assemble its public Qwen
    // runtime here so every downloaded model remains inside the selected TauriTavern data root.
    let repo = ApiBuilder::from_env()
        .with_cache_dir(cache_dir.to_path_buf())
        .with_progress(false)
        .build()?
        .model(LocalEmbeddingModel::Qwen3Embedding06B.id().to_string());

    let config: Qwen3Config = serde_json::from_slice(&std::fs::read(repo.get("config.json")?)?)?;
    let weights = [repo.get("model.safetensors")?];
    let device = Device::Cpu;
    // SAFETY: the immutable safetensors path comes from hf-hub's content-addressed cache and
    // remains owned by that cache for the lifetime of the mapped Qwen runtime.
    let variables = unsafe { VarBuilder::from_mmaped_safetensors(&weights, DType::F32, &device)? };
    let model = Qwen3Model::new(config, variables)?;
    let tokenizer_path = repo.get("tokenizer.json")?;
    let mut tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
        .map_err(|error| fastembed::Error::msg(error.to_string()))?;
    tokenizer.with_padding(Some(PaddingParams {
        strategy: PaddingStrategy::BatchLongest,
        direction: tokenizers::PaddingDirection::Left,
        ..Default::default()
    }));
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: QWEN_MAX_LENGTH,
            ..Default::default()
        }))
        .map_err(fastembed::Error::msg)?;

    Ok(Qwen3TextEmbedding::new(model, tokenizer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asymmetric_models_receive_query_and_document_prompts() {
        let qwen_query = prepare_texts(
            LocalEmbeddingModel::Qwen3Embedding06B,
            vec!["remember this".to_string()],
            true,
        );
        let qwen_document = prepare_texts(
            LocalEmbeddingModel::Qwen3Embedding06B,
            vec!["remember this".to_string()],
            false,
        );
        assert_eq!(
            qwen_query,
            [format!(
                "Instruct: {QWEN_RETRIEVAL_INSTRUCTION}\nQuery:remember this"
            )]
        );
        assert_eq!(qwen_document, ["remember this"]);

        assert_eq!(
            prepare_texts(
                LocalEmbeddingModel::EmbeddingGemma300M,
                vec!["memory".to_string()],
                true,
            ),
            ["task: search result | query: memory"]
        );
        assert_eq!(
            prepare_texts(
                LocalEmbeddingModel::EmbeddingGemma300M,
                vec!["memory".to_string()],
                false,
            ),
            ["title: none | text: memory"]
        );
    }
}
