use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen3::{Config, Model};
use tokenizers::{Tokenizer, TruncationParams};
use tokio::sync::OnceCell;

use tt_adapter_http::{HttpClientPool, PinnedFile, PinnedFileCache};
use tt_domain::errors::DomainError;
use tt_ports::repositories::vector_repository::{
    LocalEmbeddingModel, LocalEmbeddingRepository, LocalEmbeddingRequest,
};

const QWEN_MAX_LENGTH: usize = 8_192;
const QWEN_RETRIEVAL_INSTRUCTION: &str =
    "Given a conversation context, retrieve relevant passages and memories";

macro_rules! qwen3_embedding_0_6b {
    ($file:literal) => {
        concat!(
            "https://modelscope.cn/models/Qwen/Qwen3-Embedding-0.6B/resolve/dd3e37eb506b3addbd930037796e25d349ae9010/",
            $file
        )
    };
}

const QWEN3_EMBEDDING_0_6B: QwenFiles = QwenFiles {
    config: PinnedFile {
        url: qwen3_embedding_0_6b!("config.json"),
        sha256: "b5bf1f51fc45be473a54718cef92448d90a1be001bf9b9a44b8c7f10a19feaa9",
    },
    tokenizer: PinnedFile {
        url: qwen3_embedding_0_6b!("tokenizer.json"),
        sha256: "def76fb086971c7867b829c23a26261e38d9d74e02139253b38aeb9df8b4b50a",
    },
    weights: PinnedFile {
        url: qwen3_embedding_0_6b!("model.safetensors"),
        sha256: "0437e45c94563b09e13cb7a64478fc406947a93cb34a7e05870fc8dcd48e23fd",
    },
};

type EmbeddingResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct CandleLocalEmbeddingRepository {
    files: PinnedFileCache,
    qwen3_embedding_0_6b: OnceCell<Arc<QwenEmbedding>>,
}

#[derive(Clone, Copy)]
struct QwenFiles {
    config: PinnedFile,
    tokenizer: PinnedFile,
    weights: PinnedFile,
}

impl QwenFiles {
    const fn of(model: LocalEmbeddingModel) -> Self {
        match model {
            LocalEmbeddingModel::Qwen3Embedding06B => QWEN3_EMBEDDING_0_6B,
        }
    }

    const fn all(self) -> [PinnedFile; 3] {
        [self.config, self.tokenizer, self.weights]
    }
}

struct QwenEmbedding {
    /// Only run clones: they share weights but each text needs an empty KV cache.
    model: Model,
    tokenizer: Tokenizer,
}

impl QwenEmbedding {
    fn load(config: &Path, tokenizer: &Path, weights: &Path) -> EmbeddingResult<Self> {
        let config: Config = serde_json::from_slice(&std::fs::read(config)?)?;
        // SAFETY: the cache never modifies the published weights while they are mapped.
        let weights =
            unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F16, &Device::Cpu)? }
                .rename_f(|name| name.strip_prefix("model.").unwrap_or(name).to_string());
        let model = Model::new(&config, weights)?;
        let mut tokenizer = Tokenizer::from_file(tokenizer)?;
        tokenizer.with_truncation(Some(TruncationParams {
            max_length: QWEN_MAX_LENGTH,
            ..Default::default()
        }))?;
        Ok(Self { model, tokenizer })
    }

    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        let encoding = self.tokenizer.encode(text, true)?;
        let ids = encoding.get_ids();
        let input = Tensor::new(ids, &Device::Cpu)?.unsqueeze(0)?;
        let hidden = self.model.clone().forward(&input, 0)?;
        // The tokenizer ends every text with <|endoftext|>, so there is a last token.
        Ok(hidden
            .i((0, ids.len() - 1))?
            .to_dtype(DType::F32)?
            .to_vec1()?)
    }
}

impl CandleLocalEmbeddingRepository {
    pub fn new(cache_dir: PathBuf, http_clients: Arc<HttpClientPool>) -> Self {
        Self {
            files: PinnedFileCache::new(cache_dir, http_clients),
            qwen3_embedding_0_6b: OnceCell::new(),
        }
    }

    async fn runtime(&self, model: LocalEmbeddingModel) -> Result<Arc<QwenEmbedding>, DomainError> {
        let runtime = match model {
            LocalEmbeddingModel::Qwen3Embedding06B => &self.qwen3_embedding_0_6b,
        };
        runtime.get_or_try_init(|| self.load(model)).await.cloned()
    }

    async fn load(&self, model: LocalEmbeddingModel) -> Result<Arc<QwenEmbedding>, DomainError> {
        let supported = LocalEmbeddingModel::SUPPORTED.map(|model| QwenFiles::of(model).all());
        if let Err(error) = self.files.keep_only(supported.as_flattened()).await {
            tracing::warn!(%error, "Local embedding model cache keeps stale files");
        }

        let files = QwenFiles::of(model);
        let config = self.files.get(files.config).await?;
        let tokenizer = self.files.get(files.tokenizer).await?;
        let weights = self.files.get(files.weights).await?;
        tokio::task::spawn_blocking(move || QwenEmbedding::load(&config, &tokenizer, &weights))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Local embedding load task failed: {error}"))
            })?
            .map(Arc::new)
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to load local embedding model {}: {error}",
                    model.id()
                ))
            })
    }
}

#[async_trait]
impl LocalEmbeddingRepository for CandleLocalEmbeddingRepository {
    async fn embed(&self, request: LocalEmbeddingRequest) -> Result<Vec<Vec<f32>>, DomainError> {
        let LocalEmbeddingRequest {
            model,
            texts,
            is_query,
        } = request;
        let runtime = self.runtime(model).await?;
        tokio::task::spawn_blocking(move || {
            prepare_texts(texts, is_query)
                .iter()
                .map(|text| runtime.embed(text))
                .collect::<EmbeddingResult<Vec<_>>>()
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Local embedding task failed: {error}"))
        })?
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Local embedding inference failed for {}: {error}",
                model.id()
            ))
        })
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
