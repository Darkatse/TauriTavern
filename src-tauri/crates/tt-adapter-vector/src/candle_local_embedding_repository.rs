use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen3::{Config, Model};
use hf_hub::api::sync::ApiBuilder;
use tokenizers::{PaddingParams, PaddingStrategy, TruncationParams};

use tt_domain::errors::DomainError;
use tt_ports::repositories::vector_repository::{
    LocalEmbeddingModel, LocalEmbeddingRepository, LocalEmbeddingRequest,
};

const QWEN_MAX_LENGTH: usize = 8_192;
const QWEN_RETRIEVAL_INSTRUCTION: &str =
    "Given a conversation context, retrieve relevant passages and memories";

type EmbeddingResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct CandleLocalEmbeddingRepository {
    cache_dir: PathBuf,
    runtime: Arc<Mutex<Option<QwenEmbedding>>>,
}

struct QwenEmbedding {
    config: Config,
    weights: VarBuilder<'static>,
    tokenizer: tokenizers::Tokenizer,
}

impl QwenEmbedding {
    fn load(cache_dir: PathBuf) -> EmbeddingResult<Self> {
        let repo = ApiBuilder::from_env()
            .with_cache_dir(cache_dir)
            .with_progress(false)
            .build()?
            .model(LocalEmbeddingModel::Qwen3Embedding06B.id().to_string());

        let config = serde_json::from_slice(&std::fs::read(repo.get("config.json")?)?)?;
        let weight_files = [repo.get("model.safetensors")?];
        let device = Device::Cpu;
        // SAFETY: hf-hub owns the immutable, content-addressed files for the mmap lifetime.
        let weights =
            unsafe { VarBuilder::from_mmaped_safetensors(&weight_files, DType::F32, &device)? }
                .rename_f(|name| name.strip_prefix("model.").unwrap_or(name).to_string());
        let mut tokenizer = tokenizers::Tokenizer::from_file(repo.get("tokenizer.json")?)?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            direction: tokenizers::PaddingDirection::Left,
            ..Default::default()
        }));
        tokenizer.with_truncation(Some(TruncationParams {
            max_length: QWEN_MAX_LENGTH,
            ..Default::default()
        }))?;

        Ok(Self {
            config,
            weights,
            tokenizer,
        })
    }

    fn embed(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let encodings = self.tokenizer.encode_batch(texts.to_vec(), true)?;
        let batch_size = encodings.len();
        let sequence_length = encodings[0].len();
        let input_ids = encodings
            .iter()
            .flat_map(|encoding| encoding.get_ids().iter().copied())
            .collect::<Vec<_>>();
        let input = Tensor::from_vec(
            input_ids,
            (batch_size, sequence_length),
            self.weights.device(),
        )?;

        // Model keeps a generation KV cache, so recreate its lightweight tensor views per batch.
        let mut model = Model::new(&self.config, self.weights.clone())?;
        let hidden = model.forward(&input, 0)?;
        let pooled = hidden.i((.., sequence_length - 1))?;
        let norm = pooled.sqr()?.sum_keepdim(1)?.sqrt()?;
        Ok(pooled
            .broadcast_div(&norm)?
            .to_dtype(DType::F32)?
            .to_vec2()?)
    }
}

impl CandleLocalEmbeddingRepository {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            runtime: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl LocalEmbeddingRepository for CandleLocalEmbeddingRepository {
    async fn embed(&self, request: LocalEmbeddingRequest) -> Result<Vec<Vec<f32>>, DomainError> {
        let cache_dir = self.cache_dir.clone();
        let runtime = self.runtime.clone();
        tokio::task::spawn_blocking(move || {
            let texts = prepare_texts(request.texts, request.is_query);
            let mut runtime = runtime.lock().map_err(|_| {
                DomainError::InternalError("Local embedding runtime lock was poisoned".to_string())
            })?;

            if runtime.is_none() {
                *runtime = Some(QwenEmbedding::load(cache_dir).map_err(|error| {
                    DomainError::transient(format!(
                        "Failed to initialize local embedding model {}: {error}",
                        request.model.id()
                    ))
                })?);
            }

            runtime
                .as_ref()
                .expect("local embedding runtime was initialized")
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

fn prepare_texts(texts: Vec<String>, is_query: bool) -> Vec<String> {
    if !is_query {
        return texts;
    }

    texts
        .into_iter()
        .map(|text| format!("Instruct: {QWEN_RETRIEVAL_INSTRUCTION}\nQuery:{text}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_receive_the_retrieval_instruction() {
        assert_eq!(
            prepare_texts(vec!["remember this".to_string()], true),
            [format!(
                "Instruct: {QWEN_RETRIEVAL_INSTRUCTION}\nQuery:remember this"
            )]
        );
        assert_eq!(
            prepare_texts(vec!["remember this".to_string()], false),
            ["remember this"]
        );
    }
}
