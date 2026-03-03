use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use flate2::read::GzDecoder;
use miktik::{TokenizerError, TokenizerRegistry};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::domain::errors::DomainError;
use crate::domain::repositories::tokenizer_repository::TokenizerRepository;
use crate::infrastructure::http_client::build_http_client;

const CLAUDE_JSON_BYTES: &[u8] = include_bytes!("../../../resources/tokenizers/claude.json");
const GEMMA_MODEL_BYTES: &[u8] = include_bytes!("../../../resources/tokenizers/gemma.model");

#[derive(Clone, Copy)]
enum ModelSource {
    Bundled(&'static [u8]),
    Remote { url: &'static str, gzip: bool },
}

#[derive(Clone, Copy)]
struct ModelResourceSpec {
    file_name: &'static str,
    source: ModelSource,
}

pub struct MiktikTokenizerRepository {
    registry: Arc<TokenizerRegistry>,
    cache_dir: PathBuf,
    http_client: Client,
    ready_hf_models: RwLock<HashSet<&'static str>>,
    registration_guard: Mutex<()>,
}

impl MiktikTokenizerRepository {
    pub fn new(cache_dir: PathBuf) -> Result<Self, DomainError> {
        let http_client = build_http_client(
            Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60)),
        )
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to build HTTP client: {error}"))
        })?;

        let repository = Self {
            registry: Arc::new(TokenizerRegistry::new()),
            cache_dir,
            http_client,
            ready_hf_models: RwLock::new(HashSet::new()),
            registration_guard: Mutex::new(()),
        };

        Ok(repository)
    }

    fn canonical_model(requested_model: &str) -> &'static str {
        TokenizerRegistry::resolve_model_ref(requested_model)
    }

    fn model_resource_spec(canonical: &str) -> Option<ModelResourceSpec> {
        match canonical {
            "claude" => Some(ModelResourceSpec {
                file_name: "claude.json",
                source: ModelSource::Bundled(CLAUDE_JSON_BYTES),
            }),
            "gemma" => Some(ModelResourceSpec {
                file_name: "gemma.model",
                source: ModelSource::Bundled(GEMMA_MODEL_BYTES),
            }),
            "llama3" => Some(ModelResourceSpec {
                file_name: "llama3.json",
                source: ModelSource::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/llama3.json",
                    gzip: false,
                },
            }),
            "llama" => Some(ModelResourceSpec {
                file_name: "llama.model",
                source: ModelSource::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/llama.model",
                    gzip: false,
                },
            }),
            "mistral" => Some(ModelResourceSpec {
                file_name: "mistral.model",
                source: ModelSource::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/mistral.model",
                    gzip: false,
                },
            }),
            "yi" => Some(ModelResourceSpec {
                file_name: "yi.model",
                source: ModelSource::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/yi.model",
                    gzip: false,
                },
            }),
            "jamba" => Some(ModelResourceSpec {
                file_name: "jamba.model",
                source: ModelSource::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/jamba.model",
                    gzip: false,
                },
            }),
            "nerdstash" => Some(ModelResourceSpec {
                file_name: "nerdstash.model",
                source: ModelSource::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/nerdstash.model",
                    gzip: false,
                },
            }),
            "command-r" => Some(ModelResourceSpec {
                file_name: "command-r.json",
                source: ModelSource::Remote {
                    url: "https://github.com/SillyTavern/SillyTavern-Tokenizers/raw/main/command-r.json.gz",
                    gzip: true,
                },
            }),
            "command-a" => Some(ModelResourceSpec {
                file_name: "command-a.json",
                source: ModelSource::Remote {
                    url: "https://github.com/SillyTavern/SillyTavern-Tokenizers/raw/main/command-a.json.gz",
                    gzip: true,
                },
            }),
            "qwen2" => Some(ModelResourceSpec {
                file_name: "qwen2.json",
                source: ModelSource::Remote {
                    url: "https://github.com/SillyTavern/SillyTavern-Tokenizers/raw/main/qwen2.json.gz",
                    gzip: true,
                },
            }),
            "nemo" => Some(ModelResourceSpec {
                file_name: "nemo.json",
                source: ModelSource::Remote {
                    url: "https://github.com/SillyTavern/SillyTavern-Tokenizers/raw/main/nemo.json.gz",
                    gzip: true,
                },
            }),
            "deepseek" => Some(ModelResourceSpec {
                file_name: "deepseek.json",
                source: ModelSource::Remote {
                    url: "https://github.com/SillyTavern/SillyTavern-Tokenizers/raw/main/deepseek.json.gz",
                    gzip: true,
                },
            }),
            _ => None,
        }
    }

    async fn ensure_hf_model_ready(&self, canonical: &'static str) -> Result<(), DomainError> {
        if self.is_model_ready(canonical).await {
            return Ok(());
        }

        let _guard = self.registration_guard.lock().await;

        if self.is_model_ready(canonical).await {
            return Ok(());
        }

        let spec = Self::model_resource_spec(canonical).ok_or_else(|| {
            DomainError::NotFound(format!(
                "Tokenizer resource spec is missing for model '{}'",
                canonical
            ))
        })?;

        match spec.source {
            // Bundled resources are registered from bytes directly to avoid filesystem I/O.
            ModelSource::Bundled(bytes) => self
                .registry
                .register_model_bytes(canonical, bytes.to_vec())
                .map_err(|error| {
                    Self::map_tokenizer_error("register bundled model bytes", canonical, error)
                })?,
            ModelSource::Remote { .. } => {
                let model_path = self.ensure_model_file(canonical).await?;
                self.registry
                    .register_model_file(canonical, &model_path)
                    .map_err(|error| {
                        Self::map_tokenizer_error("register model resource", canonical, error)
                    })?;
            }
        }

        self.warm_model(canonical).await?;
        self.mark_model_ready(canonical).await;
        Ok(())
    }

    async fn warm_model(&self, canonical: &'static str) -> Result<(), DomainError> {
        let registry = Arc::clone(&self.registry);

        tokio::task::spawn_blocking(move || registry.get_canonical(canonical))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Tokenizer warm-up task failed for '{canonical}': {error}"
                ))
            })?
            .map_err(|error| Self::map_tokenizer_error("load tokenizer", canonical, error))?;

        Ok(())
    }

    async fn ensure_model_file(&self, canonical: &'static str) -> Result<PathBuf, DomainError> {
        let spec = Self::model_resource_spec(canonical).ok_or_else(|| {
            DomainError::NotFound(format!(
                "Tokenizer resource spec is missing for model '{}'",
                canonical
            ))
        })?;

        let path = self.cache_dir.join(spec.file_name);
        if path.exists() {
            return Ok(path);
        }

        let bytes = match spec.source {
            ModelSource::Bundled(bytes) => bytes.to_vec(),
            ModelSource::Remote { url, gzip } => self.download_model_bytes(url, gzip).await?,
        };

        self.write_bytes(&path, &bytes).await?;
        Ok(path)
    }

    async fn download_model_bytes(&self, url: &str, gzip: bool) -> Result<Vec<u8>, DomainError> {
        let response = self
            .http_client
            .get(url)
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to download tokenizer resource '{}': {}",
                    url, error
                ))
            })?
            .error_for_status()
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Tokenizer resource request failed for '{}': {}",
                    url, error
                ))
            })?;

        let payload = response.bytes().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read downloaded tokenizer bytes from '{}': {}",
                url, error
            ))
        })?;

        let payload = payload.to_vec();
        if !gzip {
            return Ok(payload);
        }

        let mut decoder = GzDecoder::new(Cursor::new(payload));
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to decompress tokenizer payload '{}': {}",
                url, error
            ))
        })?;

        Ok(decompressed)
    }

    async fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), DomainError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create tokenizer cache directory '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        tokio::fs::write(path, bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to persist tokenizer resource to '{}': {}",
                path.display(),
                error
            ))
        })?;

        Ok(())
    }

    async fn is_model_ready(&self, canonical: &'static str) -> bool {
        self.ready_hf_models.read().await.contains(canonical)
    }

    async fn mark_model_ready(&self, canonical: &'static str) {
        self.ready_hf_models.write().await.insert(canonical);
    }

    fn map_tokenizer_error(action: &str, model: &str, error: TokenizerError) -> DomainError {
        match error {
            TokenizerError::ModelNotFound(message) => {
                DomainError::NotFound(format!("Failed to {} for '{}': {}", action, model, message))
            }
            TokenizerError::LoadError(message)
            | TokenizerError::EncodeError(message)
            | TokenizerError::DecodeError(message) => DomainError::InternalError(format!(
                "Failed to {} for '{}': {}",
                action, model, message
            )),
        }
    }

    fn value_to_text(value: &Value) -> Cow<'_, str> {
        match value {
            Value::String(text) => Cow::Borrowed(text),
            _ => Cow::Owned(value.to_string()),
        }
    }

    fn to_sentencepiece_count_input(messages: &[Value]) -> String {
        let mut values = Vec::new();
        for message in messages {
            match message {
                Value::Object(map) => {
                    for value in map.values() {
                        values.push(Self::value_to_text(value).into_owned());
                    }
                }
                _ => values.push(Self::value_to_text(message).into_owned()),
            }
        }
        values.join("\n\n")
    }

    fn to_web_tokenizer_prompt(messages: &[Value]) -> String {
        #[derive(Clone)]
        struct PromptMessage {
            role: String,
            name: Option<String>,
            content: String,
        }

        let mut mapped = messages
            .iter()
            .map(|value| match value {
                Value::Object(map) => {
                    let role = map
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("system")
                        .to_string();
                    let name = map.get("name").and_then(Value::as_str).map(str::to_string);
                    let mut content = map
                        .get("content")
                        .map(Self::value_to_text)
                        .map(Cow::into_owned)
                        .unwrap_or_default();
                    if let Some(tool_calls) = map.get("tool_calls") {
                        content.push_str(&tool_calls.to_string());
                    }
                    PromptMessage {
                        role,
                        name,
                        content,
                    }
                }
                _ => PromptMessage {
                    role: "system".to_string(),
                    name: None,
                    content: Self::value_to_text(value).into_owned(),
                },
            })
            .collect::<Vec<_>>();

        if !mapped.is_empty() {
            mapped[0].role = "system".to_string();

            let mut first_assistant_index = None;
            for (index, message) in mapped.iter().enumerate() {
                if index > 0 && message.role == "assistant" {
                    first_assistant_index = Some(index);
                    break;
                }
            }

            // Mirrors SillyTavern's convertClaudePrompt fixed-parameter path used in token counting.
            mapped[0].role = "user".to_string();
            if let Some(index) = first_assistant_index {
                let candidate_index = index.saturating_sub(1);
                if candidate_index != 0 && mapped[candidate_index].role == "user" {
                    mapped[candidate_index].role = "FixHumMsg".to_string();
                }
            }
        }

        let mut prompt = String::new();
        for (index, message) in mapped.iter().enumerate() {
            let prefix = match message.role.as_str() {
                "assistant" => "\n\nAssistant: ",
                "user" => "\n\nHuman: ",
                "system" => {
                    if index == 0 {
                        ""
                    } else if message.name.as_deref() == Some("example_assistant") {
                        "\n\nA: "
                    } else if message.name.as_deref() == Some("example_user") {
                        "\n\nH: "
                    } else {
                        "\n\n"
                    }
                }
                "FixHumMsg" => "\n\nFirst message: ",
                _ => "",
            };

            prompt.push_str(prefix);

            if message.role != "system" {
                if let Some(name) = message.name.as_deref() {
                    if !name.is_empty() {
                        prompt.push_str(name);
                        prompt.push_str(": ");
                    }
                }
            }

            prompt.push_str(&message.content);
        }

        prompt
    }

    fn count_openai_messages(
        &self,
        canonical: &'static str,
        messages: &[Value],
    ) -> Result<usize, DomainError> {
        let is_legacy = canonical == "gpt-3.5-turbo-0301";
        let tokens_per_message = if is_legacy { 4_i32 } else { 3_i32 };
        let tokens_per_name = if is_legacy { -1_i32 } else { 1_i32 };
        let tokenizer = self
            .registry
            .get_canonical(canonical)
            .map_err(|error| Self::map_tokenizer_error("load tokenizer", canonical, error))?;
        let mut total = 0_i32;

        for message in messages {
            total += tokens_per_message;

            match message {
                Value::Object(map) => {
                    for (key, value) in map {
                        let text = Self::value_to_text(value);
                        let count = tokenizer.count_tokens(text.as_ref()).map_err(|error| {
                            Self::map_tokenizer_error("count tokens", canonical, error)
                        })?;
                        total += count as i32;
                        if key == "name" {
                            total += tokens_per_name;
                        }
                    }
                }
                _ => {
                    let text = Self::value_to_text(message);
                    let count = tokenizer.count_tokens(text.as_ref()).map_err(|error| {
                        Self::map_tokenizer_error("count tokens", canonical, error)
                    })?;
                    total += count as i32;
                }
            }
        }

        total += 3;
        if is_legacy {
            total += 9;
        }

        Ok(total.max(0) as usize)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::MiktikTokenizerRepository;
    use crate::domain::repositories::tokenizer_repository::TokenizerRepository;

    #[test]
    fn canonical_model_aligns_sillytavern_aliases() {
        assert_eq!(
            MiktikTokenizerRepository::canonical_model("gpt-4.1-mini"),
            "gpt-4o"
        );
        assert_eq!(MiktikTokenizerRepository::canonical_model("o4-mini"), "o1");
        assert_eq!(
            MiktikTokenizerRepository::canonical_model("gemini-2.0-flash"),
            "gemma"
        );
        assert_eq!(
            MiktikTokenizerRepository::canonical_model("claude-3-7-sonnet"),
            "claude"
        );
    }

    #[test]
    fn sentencepiece_count_input_flattens_all_message_values() {
        let messages = vec![
            json!({"role": "user", "content": "hello", "name": "Alice"}),
            json!("tail"),
        ];
        let input = MiktikTokenizerRepository::to_sentencepiece_count_input(&messages);
        assert!(input.contains("user"));
        assert!(input.contains("hello"));
        assert!(input.contains("Alice"));
        assert!(input.ends_with("tail"));
        assert_eq!(input.matches("\n\n").count(), 3);
    }

    #[test]
    fn web_tokenizer_prompt_uses_claude_prefixes() {
        let messages = vec![
            json!({"role": "system", "content": "sys"}),
            json!({"role": "user", "content": "hello"}),
            json!({"role": "assistant", "content": "world"}),
        ];
        let prompt = MiktikTokenizerRepository::to_web_tokenizer_prompt(&messages);
        assert!(prompt.contains("\n\nHuman: sys"));
        assert!(prompt.contains("\n\nFirst message: hello"));
        assert!(prompt.contains("\n\nAssistant: world"));
    }

    fn unique_temp_cache_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tauritavern-tokenizer-test-{}-{nonce}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn bundled_models_are_usable_without_network() {
        let cache_dir = unique_temp_cache_dir();
        let repository = MiktikTokenizerRepository::new(cache_dir.clone())
            .expect("repository should initialize with bundled models");
        let messages = vec![json!({"role": "user", "content": "hello world"})];

        TokenizerRepository::ensure_model_ready(&repository, "claude-3-7-sonnet")
            .await
            .expect("claude bundled tokenizer should prepare");
        TokenizerRepository::ensure_model_ready(&repository, "gemini-2.0-flash")
            .await
            .expect("gemma bundled tokenizer should prepare");

        let claude =
            TokenizerRepository::count_messages(&repository, "claude-3-7-sonnet", &messages)
                .expect("claude bundled tokenizer should count");
        let gemini =
            TokenizerRepository::count_messages(&repository, "gemini-2.0-flash", &messages)
                .expect("gemma bundled tokenizer should count");

        let _ = std::fs::remove_dir_all(cache_dir);
        assert!(claude > 0);
        assert!(gemini > 0);
    }

    #[tokio::test]
    async fn new_does_not_eagerly_register_bundled_models() {
        let cache_dir = unique_temp_cache_dir();
        let repository = MiktikTokenizerRepository::new(cache_dir.clone())
            .expect("repository should initialize");

        assert!(!repository.is_model_ready("claude").await);
        assert!(!repository.is_model_ready("gemma").await);
        let _ = std::fs::remove_dir_all(cache_dir);
    }

    #[tokio::test]
    async fn bundled_models_do_not_write_cache_files_on_first_use() {
        let cache_dir = unique_temp_cache_dir();
        let repository = MiktikTokenizerRepository::new(cache_dir.clone())
            .expect("repository should initialize");
        let messages = vec![json!({"role": "user", "content": "hello world"})];

        TokenizerRepository::ensure_model_ready(&repository, "claude")
            .await
            .expect("claude bundled tokenizer should prepare");
        TokenizerRepository::ensure_model_ready(&repository, "gemma")
            .await
            .expect("gemma bundled tokenizer should prepare");

        TokenizerRepository::count_messages(&repository, "claude", &messages)
            .expect("claude bundled tokenizer should count");
        TokenizerRepository::count_messages(&repository, "gemma", &messages)
            .expect("gemma bundled tokenizer should count");

        assert!(
            !cache_dir.join("claude.json").exists(),
            "claude bundled tokenizer should not be materialized to cache"
        );
        assert!(
            !cache_dir.join("gemma.model").exists(),
            "gemma bundled tokenizer should not be materialized to cache"
        );
        let _ = std::fs::remove_dir_all(cache_dir);
    }
}

impl Default for MiktikTokenizerRepository {
    fn default() -> Self {
        let fallback_cache = std::env::temp_dir().join("tauritavern-tokenizers");
        Self::new(fallback_cache).expect("failed to initialize MiktikTokenizerRepository")
    }
}

#[async_trait::async_trait]
impl TokenizerRepository for MiktikTokenizerRepository {
    async fn ensure_model_ready(&self, model: &str) -> Result<(), DomainError> {
        let canonical = Self::canonical_model(model);
        if TokenizerRegistry::is_huggingface_model(canonical) {
            self.ensure_hf_model_ready(canonical).await?;
        }
        Ok(())
    }

    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError> {
        let canonical = Self::canonical_model(model);
        let tokenizer = self
            .registry
            .get_canonical(canonical)
            .map_err(|error| Self::map_tokenizer_error("load tokenizer", canonical, error))?;

        tokenizer
            .encode(text)
            .map_err(|error| Self::map_tokenizer_error("encode text", canonical, error))
    }

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError> {
        let canonical = Self::canonical_model(model);
        let tokenizer = self
            .registry
            .get_canonical(canonical)
            .map_err(|error| Self::map_tokenizer_error("load tokenizer", canonical, error))?;

        tokenizer
            .decode(token_ids)
            .map_err(|error| Self::map_tokenizer_error("decode token ids", canonical, error))
    }

    fn count_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError> {
        let canonical = Self::canonical_model(model);

        if TokenizerRegistry::is_sentencepiece_model(canonical) {
            let text = Self::to_sentencepiece_count_input(messages);
            return self
                .registry
                .count_tokens_canonical(canonical, &text)
                .map_err(|error| {
                    Self::map_tokenizer_error("count sentencepiece messages", canonical, error)
                });
        }

        if TokenizerRegistry::is_web_tokenizer_model(canonical) {
            let prompt = Self::to_web_tokenizer_prompt(messages);
            return self
                .registry
                .count_tokens_canonical(canonical, &prompt)
                .map_err(|error| {
                    Self::map_tokenizer_error("count web-tokenizer messages", canonical, error)
                });
        }

        self.count_openai_messages(canonical, messages)
    }
}
