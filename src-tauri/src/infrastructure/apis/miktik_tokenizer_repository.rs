use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use flate2::read::GzDecoder;
use miktik::{TokenizerError, TokenizerRegistry};
use serde_json::Value;
use ureq::Agent;

use crate::domain::errors::DomainError;
use crate::domain::repositories::tokenizer_repository::TokenizerRepository;

const DEFAULT_FALLBACK_MODEL: &str = "gpt-3.5-turbo";
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
    registry: TokenizerRegistry,
    cache_dir: PathBuf,
    http_client: Agent,
    registered_hf_models: RwLock<HashSet<String>>,
    registration_guard: Mutex<()>,
}

impl MiktikTokenizerRepository {
    pub fn new(cache_dir: PathBuf) -> Result<Self, DomainError> {
        let http_client: Agent = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();

        let repository = Self {
            registry: TokenizerRegistry::new(),
            cache_dir,
            http_client,
            registered_hf_models: RwLock::new(HashSet::new()),
            registration_guard: Mutex::new(()),
        };

        Ok(repository)
    }

    fn prepare_model(&self, requested_model: &str) -> Result<String, DomainError> {
        let canonical = Self::canonical_model(requested_model);
        if Self::is_huggingface_model(&canonical) {
            self.ensure_hf_model_registered(&canonical)?;
        }
        Ok(canonical)
    }

    fn canonical_model(requested_model: &str) -> String {
        let model = requested_model.trim().to_ascii_lowercase();

        if model.is_empty() {
            return DEFAULT_FALLBACK_MODEL.to_string();
        }

        // Keep OpenAI aliases aligned with SillyTavern tokenizer routing.
        if model == "o1"
            || model.contains("o1-preview")
            || model.contains("o1-mini")
            || model.contains("o3-mini")
        {
            return "o1".to_string();
        }

        if model.contains("gpt-5") || model.contains("o3") || model.contains("o4-mini") {
            return "o1".to_string();
        }

        if model.contains("gpt-4o")
            || model.contains("chatgpt-4o-latest")
            || model.contains("gpt-4.1")
            || model.contains("gpt-4.5")
        {
            return "gpt-4o".to_string();
        }

        if model.contains("gpt-4-32k") {
            return "gpt-4-32k".to_string();
        }

        if model.contains("gpt-4") {
            return "gpt-4".to_string();
        }

        if model.contains("gpt-3.5-turbo-0301") {
            return "gpt-3.5-turbo-0301".to_string();
        }

        if model.contains("gpt-3.5-turbo") {
            return "gpt-3.5-turbo".to_string();
        }

        if model.contains("claude") {
            return "claude".to_string();
        }

        if model.contains("llama3") || model.contains("llama-3") {
            return "llama3".to_string();
        }

        if model.contains("llama") {
            return "llama".to_string();
        }

        if model.contains("mistral") {
            return "mistral".to_string();
        }

        if model.contains("yi") {
            return "yi".to_string();
        }

        if model.contains("deepseek") {
            return "deepseek".to_string();
        }

        if model.contains("gemma") || model.contains("gemini") || model.contains("learnlm") {
            return "gemma".to_string();
        }

        if model.contains("jamba") {
            return "jamba".to_string();
        }

        if model.contains("qwen2") || model.contains("qwen") {
            return "qwen2".to_string();
        }

        if model.contains("command-r") {
            return "command-r".to_string();
        }

        if model.contains("command-a") {
            return "command-a".to_string();
        }

        if model.contains("nemo") || model.contains("pixtral") {
            return "nemo".to_string();
        }

        if model.contains("nerdstash") {
            return "nerdstash".to_string();
        }

        TokenizerRegistry::resolve_model(&model)
    }

    fn is_huggingface_model(canonical: &str) -> bool {
        matches!(
            canonical,
            "claude"
                | "llama3"
                | "llama"
                | "mistral"
                | "yi"
                | "gemma"
                | "jamba"
                | "nerdstash"
                | "command-r"
                | "command-a"
                | "qwen2"
                | "nemo"
                | "deepseek"
        )
    }

    fn is_sentencepiece_model(canonical: &str) -> bool {
        matches!(
            canonical,
            "llama" | "mistral" | "yi" | "gemma" | "jamba" | "nerdstash"
        )
    }

    fn is_web_tokenizer_model(canonical: &str) -> bool {
        matches!(
            canonical,
            "claude" | "llama3" | "command-r" | "command-a" | "qwen2" | "nemo" | "deepseek"
        )
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

    fn ensure_hf_model_registered(&self, canonical: &str) -> Result<(), DomainError> {
        if self.is_model_registered(canonical)? {
            return Ok(());
        }

        let _guard = self.registration_guard.lock().map_err(|error| {
            DomainError::InternalError(format!(
                "Tokenizer registration lock poisoned for model '{}': {}",
                canonical, error
            ))
        })?;

        if self.is_model_registered(canonical)? {
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
                let model_path = self.ensure_model_file(canonical)?;
                self.registry
                    .register_model_file(canonical, &model_path)
                    .map_err(|error| {
                        Self::map_tokenizer_error("register model resource", canonical, error)
                    })?;
            }
        }

        self.mark_model_registered(canonical)?;
        Ok(())
    }

    fn ensure_model_file(&self, canonical: &str) -> Result<PathBuf, DomainError> {
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
            ModelSource::Remote { url, gzip } => self.download_model_bytes(url, gzip)?,
        };

        self.write_bytes(&path, &bytes)?;
        Ok(path)
    }

    fn download_model_bytes(&self, url: &str, gzip: bool) -> Result<Vec<u8>, DomainError> {
        let mut response = self.http_client.get(url).call().map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to download tokenizer resource '{}': {}",
                url, error
            ))
        })?;

        if !response.status().is_success() {
            return Err(DomainError::InternalError(format!(
                "Tokenizer resource request failed for '{}': HTTP {}",
                url,
                response.status()
            )));
        }

        let mut payload = Vec::new();
        response
            .body_mut()
            .as_reader()
            .read_to_end(&mut payload)
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read downloaded tokenizer bytes from '{}': {}",
                    url, error
                ))
            })?;

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

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), DomainError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create tokenizer cache directory '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        fs::write(path, bytes).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to persist tokenizer resource to '{}': {}",
                path.display(),
                error
            ))
        })
    }

    fn is_model_registered(&self, canonical: &str) -> Result<bool, DomainError> {
        let registered = self.registered_hf_models.read().map_err(|error| {
            DomainError::InternalError(format!(
                "Tokenizer registration cache read lock failed: {}",
                error
            ))
        })?;
        Ok(registered.contains(canonical))
    }

    fn mark_model_registered(&self, canonical: &str) -> Result<(), DomainError> {
        let mut registered = self.registered_hf_models.write().map_err(|error| {
            DomainError::InternalError(format!(
                "Tokenizer registration cache write lock failed: {}",
                error
            ))
        })?;
        registered.insert(canonical.to_string());
        Ok(())
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

    fn value_to_text(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            _ => value.to_string(),
        }
    }

    fn to_sentencepiece_count_input(messages: &[Value]) -> String {
        let mut values = Vec::new();
        for message in messages {
            match message {
                Value::Object(map) => {
                    for value in map.values() {
                        values.push(Self::value_to_text(value));
                    }
                }
                _ => values.push(Self::value_to_text(message)),
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
                    content: Self::value_to_text(value),
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

    fn count_openai_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError> {
        let is_legacy = model.contains("gpt-3.5-turbo-0301");
        let tokens_per_message = if is_legacy { 4_i32 } else { 3_i32 };
        let tokens_per_name = if is_legacy { -1_i32 } else { 1_i32 };
        let mut total = 0_i32;

        for message in messages {
            total += tokens_per_message;

            match message {
                Value::Object(map) => {
                    for (key, value) in map {
                        let text = Self::value_to_text(value);
                        let count = self.registry.count_tokens(model, &text).map_err(|error| {
                            Self::map_tokenizer_error("count tokens", model, error)
                        })?;
                        total += count as i32;
                        if key == "name" {
                            total += tokens_per_name;
                        }
                    }
                }
                _ => {
                    let text = Self::value_to_text(message);
                    let count = self
                        .registry
                        .count_tokens(model, &text)
                        .map_err(|error| Self::map_tokenizer_error("count tokens", model, error))?;
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

    #[test]
    fn bundled_models_are_usable_without_network() {
        let cache_dir = unique_temp_cache_dir();
        let repository = MiktikTokenizerRepository::new(cache_dir.clone())
            .expect("repository should initialize with bundled models");
        let messages = vec![json!({"role": "user", "content": "hello world"})];

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

    #[test]
    fn new_does_not_eagerly_register_bundled_models() {
        let cache_dir = unique_temp_cache_dir();
        let repository = MiktikTokenizerRepository::new(cache_dir.clone())
            .expect("repository should initialize");

        assert!(!repository
            .is_model_registered("claude")
            .expect("registration state should be readable"));
        assert!(!repository
            .is_model_registered("gemma")
            .expect("registration state should be readable"));
        let _ = std::fs::remove_dir_all(cache_dir);
    }

    #[test]
    fn bundled_models_do_not_write_cache_files_on_first_use() {
        let cache_dir = unique_temp_cache_dir();
        let repository = MiktikTokenizerRepository::new(cache_dir.clone())
            .expect("repository should initialize");
        let messages = vec![json!({"role": "user", "content": "hello world"})];

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

impl TokenizerRepository for MiktikTokenizerRepository {
    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError> {
        let canonical = self.prepare_model(model)?;
        let tokenizer = self
            .registry
            .get(&canonical)
            .map_err(|error| Self::map_tokenizer_error("load tokenizer", &canonical, error))?;

        tokenizer
            .encode(text)
            .map_err(|error| Self::map_tokenizer_error("encode text", &canonical, error))
    }

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError> {
        let canonical = self.prepare_model(model)?;
        let tokenizer = self
            .registry
            .get(&canonical)
            .map_err(|error| Self::map_tokenizer_error("load tokenizer", &canonical, error))?;

        tokenizer
            .decode(token_ids)
            .map_err(|error| Self::map_tokenizer_error("decode token ids", &canonical, error))
    }

    fn count_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError> {
        let canonical = self.prepare_model(model)?;

        if Self::is_sentencepiece_model(&canonical) {
            let text = Self::to_sentencepiece_count_input(messages);
            return self
                .registry
                .count_tokens(&canonical, &text)
                .map_err(|error| {
                    Self::map_tokenizer_error("count sentencepiece messages", &canonical, error)
                });
        }

        if Self::is_web_tokenizer_model(&canonical) {
            let prompt = Self::to_web_tokenizer_prompt(messages);
            return self
                .registry
                .count_tokens(&canonical, &prompt)
                .map_err(|error| {
                    Self::map_tokenizer_error("count web-tokenizer messages", &canonical, error)
                });
        }

        self.count_openai_messages(&canonical, messages)
    }
}
