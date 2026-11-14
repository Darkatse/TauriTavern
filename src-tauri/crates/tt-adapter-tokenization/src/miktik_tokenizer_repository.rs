use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{self, Cursor, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use miktik::{
    ARTIFACT_VERSION, Framing, Model, Source, Tokenizer, TokenizerError, TokenizerRegistry,
};
use serde_json::Value;
use tokio::sync::Mutex;

use tt_adapter_http::{HttpClientPool, HttpClientProfile, upstream};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;

const CLAUDE_JSON_GZIP_BYTES: &[u8] =
    include_bytes!("../../../resources/tokenizers/claude.json.gz");
const DEEPSEEK_JSON_GZIP_BYTES: &[u8] =
    include_bytes!("../../../resources/tokenizers/deepseek.json.gz");
const GEMMA3_MODEL_GZIP_BYTES: &[u8] =
    include_bytes!("../../../resources/tokenizers/gemma3.model.gz");

/// The tokenizer for names MikTik resolves to no model.
const UNKNOWN_MODEL_TOKENIZER: &str = "gpt-4o";
/// The bytes of segment text whose estimates each tokenizer keeps.
const SEGMENT_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
enum ResourceCompression {
    None,
    Gzip,
}

#[derive(Clone, Copy)]
enum SourceOrigin {
    /// An OpenAI encoding built into MikTik.
    Builtin,
    /// Bytes built into TauriTavern.
    Bundled {
        bytes: &'static [u8],
        compression: ResourceCompression,
    },
    /// A file downloaded on first use and kept in the cache directory under the source's name.
    Remote {
        url: &'static str,
        compression: ResourceCompression,
    },
}

/// Where a model's tokenizer comes from.
#[derive(Clone, Copy)]
struct TokenizerSource {
    /// The encoding's name, or the source's file name. A model's artifact is named after its
    /// source, so a source under a new name makes the old artifact stale.
    name: &'static str,
    origin: SourceOrigin,
}

/// Tokenizers from MikTik, kept as artifacts in the cache directory.
///
/// A model's tokenizer is compiled from its source once, written as an artifact, and loaded from
/// the artifact from then on: in milliseconds, and holding only what estimates need until an
/// exact method first runs.
pub struct MiktikTokenizerRepository {
    cache_dir: PathBuf,
    http_clients: Arc<HttpClientPool>,
    /// The prepared tokenizers by model id.
    tokenizers: RwLock<HashMap<&'static str, Arc<Tokenizer>>>,
    /// Held while preparing a tokenizer; true once the cache was swept of stale files.
    preparation: Mutex<bool>,
}

impl MiktikTokenizerRepository {
    pub fn new(cache_dir: PathBuf, http_clients: Arc<HttpClientPool>) -> Self {
        Self {
            cache_dir,
            http_clients,
            tokenizers: RwLock::new(HashMap::new()),
            preparation: Mutex::new(false),
        }
    }

    fn resolve(name: &str) -> &'static Model {
        Model::resolve(name)
            .or_else(|| Model::resolve(UNKNOWN_MODEL_TOKENIZER))
            .expect("the tokenizer for unknown names is a row of MikTik's model table")
    }

    fn source(model: &Model) -> Option<TokenizerSource> {
        let encoding = match model.source() {
            Source::Builtin(encoding) => encoding,
            Source::Registered(_) => return Self::registered_source(model.id()),
        };
        Some(TokenizerSource {
            name: encoding.name(),
            origin: SourceOrigin::Builtin,
        })
    }

    fn registered_source(id: &str) -> Option<TokenizerSource> {
        let (name, origin) = match id {
            "claude" => (
                "claude.json",
                SourceOrigin::Bundled {
                    bytes: CLAUDE_JSON_GZIP_BYTES,
                    compression: ResourceCompression::Gzip,
                },
            ),
            "deepseek" => (
                "deepseek.json",
                SourceOrigin::Bundled {
                    bytes: DEEPSEEK_JSON_GZIP_BYTES,
                    compression: ResourceCompression::Gzip,
                },
            ),
            "gemma" => (
                "gemma3.model",
                SourceOrigin::Bundled {
                    bytes: GEMMA3_MODEL_GZIP_BYTES,
                    compression: ResourceCompression::Gzip,
                },
            ),
            "kimi" => (
                "kimi.tiktoken",
                SourceOrigin::Remote {
                    url: "https://huggingface.co/moonshotai/Kimi-K2-Instruct/resolve/main/tiktoken.model",
                    compression: ResourceCompression::None,
                },
            ),
            "llama3" => (
                "llama3.json",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/llama3.json",
                    compression: ResourceCompression::None,
                },
            ),
            "llama" => (
                "llama.model",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/llama.model",
                    compression: ResourceCompression::None,
                },
            ),
            "mistral" => (
                "mistral.model",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/mistral.model",
                    compression: ResourceCompression::None,
                },
            ),
            "yi" => (
                "yi.model",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/yi.model",
                    compression: ResourceCompression::None,
                },
            ),
            "jamba" => (
                "jamba.model",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/jamba.model",
                    compression: ResourceCompression::None,
                },
            ),
            "nerdstash" => (
                "nerdstash.model",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/nerdstash.model",
                    compression: ResourceCompression::None,
                },
            ),
            "nerdstash_v2" => (
                "nerdstash_v2.model",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/src/tokenizers/nerdstash_v2.model",
                    compression: ResourceCompression::None,
                },
            ),
            "command-r" => (
                "command-r.json",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern-Tokenizers/main/command-r.json.gz",
                    compression: ResourceCompression::Gzip,
                },
            ),
            "command-a" => (
                "command-a.json",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern-Tokenizers/main/command-a.json.gz",
                    compression: ResourceCompression::Gzip,
                },
            ),
            "qwen2" => (
                "qwen2.json",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern-Tokenizers/main/qwen2.json.gz",
                    compression: ResourceCompression::Gzip,
                },
            ),
            "nemo" => (
                "nemo.json",
                SourceOrigin::Remote {
                    url: "https://raw.githubusercontent.com/SillyTavern/SillyTavern-Tokenizers/main/nemo.json.gz",
                    compression: ResourceCompression::Gzip,
                },
            ),
            _ => return None,
        };
        Some(TokenizerSource { name, origin })
    }

    fn artifact_path(&self, model: &Model, source: TokenizerSource) -> PathBuf {
        self.cache_dir
            .join("compiled")
            .join(format!("v{ARTIFACT_VERSION}"))
            .join(format!("{}-{}.mtk", model.id(), source.name))
    }

    /// The prepared tokenizer of the model the name resolves to.
    fn tokenizer(&self, name: &str) -> Result<Arc<Tokenizer>, DomainError> {
        let id = Self::resolve(name).id();
        self.read_tokenizers().get(id).cloned().ok_or_else(|| {
            DomainError::InternalError(format!("The tokenizer for '{id}' is not prepared"))
        })
    }

    fn read_tokenizers(&self) -> RwLockReadGuard<'_, HashMap<&'static str, Arc<Tokenizer>>> {
        // Entries are only inserted or removed whole, so the map is valid after a panic.
        self.tokenizers
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn write_tokenizers(&self) -> RwLockWriteGuard<'_, HashMap<&'static str, Arc<Tokenizer>>> {
        self.tokenizers
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Loads the model's artifact, compiling it first when there is none or it does not load.
    async fn prepare(&self, model: &'static Model) -> Result<Arc<Tokenizer>, DomainError> {
        let id = model.id();
        let source = Self::source(model).ok_or_else(|| {
            DomainError::NotFound(format!("No tokenizer source is known for '{id}'"))
        })?;
        let artifact_path = self.artifact_path(model, source);

        match tokio::fs::read(&artifact_path).await {
            Ok(artifact) => {
                match blocking("Tokenizer load", move || load(model, artifact)).await? {
                    Ok(tokenizer) => return Ok(tokenizer),
                    Err(error) => tracing::warn!(
                        model = id,
                        %error,
                        "Rebuilding a tokenizer artifact that does not load"
                    ),
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read tokenizer artifact '{}': {error}",
                    artifact_path.display()
                )));
            }
        }

        let artifact = self.compile(model, source).await?;
        self.write_bytes(&artifact_path, &artifact).await?;
        blocking("Tokenizer load", move || load(model, artifact))
            .await?
            .map_err(|error| tokenizer_error("load the compiled tokenizer", id, error))
    }

    /// The artifact of the model's tokenizer, converted from its source.
    async fn compile(
        &self,
        model: &'static Model,
        source: TokenizerSource,
    ) -> Result<Vec<u8>, DomainError> {
        let (url, compression) = match source.origin {
            SourceOrigin::Builtin => {
                return blocking("Tokenizer compile", move || build_artifact(model, None)).await?;
            }
            SourceOrigin::Bundled { bytes, compression } => {
                return blocking("Tokenizer compile", move || {
                    let source_bytes = decompress(bytes, compression, source.name)?;
                    build_artifact(model, Some(source_bytes))
                })
                .await?;
            }
            SourceOrigin::Remote { url, compression } => (url, compression),
        };

        let cached_path = self.cache_dir.join(source.name);
        match tokio::fs::read(&cached_path).await {
            Ok(cached) => {
                match blocking("Tokenizer compile", move || {
                    build_artifact(model, Some(cached))
                })
                .await?
                {
                    Ok(artifact) => return Ok(artifact),
                    Err(error) => {
                        tracing::warn!(
                            model = model.id(),
                            %error,
                            "Downloading again a tokenizer source that does not convert"
                        );
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read tokenizer source '{}': {error}",
                    cached_path.display()
                )));
            }
        }

        let payload = self.download(url).await?;
        let source_bytes = decompress(&payload, compression, url)?;
        self.write_bytes(&cached_path, &source_bytes).await?;
        blocking("Tokenizer compile", move || {
            build_artifact(model, Some(source_bytes))
        })
        .await?
    }

    async fn download(&self, url: &str) -> Result<Vec<u8>, DomainError> {
        let http_client = self.http_clients.client(HttpClientProfile::Tokenizer)?;
        let response = http_client
            .get(url)
            .send()
            .await
            .map_err(upstream::request_failure)?
            .error_for_status()
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Tokenizer resource request failed for '{}': {}",
                    url, error
                ))
            })?;

        let payload = upstream::read_body(response).await?;

        Ok(payload.to_vec())
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

        let temp_path = Self::temp_cache_path(path);
        tokio::fs::write(&temp_path, bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to persist tokenizer resource to '{}': {}",
                temp_path.display(),
                error
            ))
        })?;

        if let Err(error) = tokio::fs::rename(&temp_path, path).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(DomainError::InternalError(format!(
                "Failed to publish tokenizer resource to '{}': {}",
                path.display(),
                error
            )));
        }

        Ok(())
    }

    fn temp_cache_path(path: &Path) -> PathBuf {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tokenizer");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce))
    }

    /// Removes what this version does not use from the cache directory: artifacts of another
    /// artifact version or another source, sources no model downloads, and interrupted writes.
    async fn remove_stale_files(&self) -> Result<(), DomainError> {
        let mut used = HashSet::new();
        for model in Model::all() {
            let Some(source) = Self::source(model) else {
                continue;
            };
            used.insert(self.artifact_path(model, source));
            if let SourceOrigin::Remote { .. } = source.origin {
                used.insert(self.cache_dir.join(source.name));
            }
        }
        let cache_dir = self.cache_dir.clone();
        blocking("Tokenizer cache sweep", move || {
            remove_unused(&cache_dir, &used)
        })
        .await?
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to remove stale tokenizer cache files: {error}"
            ))
        })
    }

    /// Forgets a tokenizer whose artifact holds a definition MikTik cannot read, so that the next
    /// preparation compiles it again.
    fn discard(&self, model: &Model) {
        self.write_tokenizers().remove(model.id());
        let Some(source) = Self::source(model) else {
            return;
        };
        let path = self.artifact_path(model, source);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                path = %path.display(),
                %error,
                "Failed to remove an unusable tokenizer artifact"
            ),
        }
    }

    /// The error of an exact method. An artifact whose definition turns out unusable is
    /// discarded, so that the next call works again.
    fn exact_error(
        &self,
        action: &str,
        tokenizer: &Tokenizer,
        error: TokenizerError,
    ) -> DomainError {
        if matches!(error, TokenizerError::Artifact(_)) {
            self.discard(tokenizer.model());
        }
        tokenizer_error(action, tokenizer.model().id(), error)
    }
}

#[async_trait::async_trait]
impl TokenizerRepository for MiktikTokenizerRepository {
    async fn ensure_model_ready(&self, name: &str) -> Result<(), DomainError> {
        let model = Self::resolve(name);
        if self.read_tokenizers().contains_key(model.id()) {
            return Ok(());
        }

        let mut swept = self.preparation.lock().await;
        if !*swept {
            if let Err(error) = self.remove_stale_files().await {
                tracing::warn!(%error, "Tokenizer cache keeps stale files");
            }
            *swept = true;
        }
        if self.read_tokenizers().contains_key(model.id()) {
            return Ok(());
        }

        let tokenizer = self.prepare(model).await?;
        self.write_tokenizers().insert(model.id(), tokenizer);
        Ok(())
    }

    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError> {
        let tokenizer = self.tokenizer(model)?;
        tokenizer
            .encode(text)
            .map_err(|error| self.exact_error("encode text", &tokenizer, error))
    }

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError> {
        let tokenizer = self.tokenizer(model)?;
        tokenizer
            .decode(token_ids)
            .map_err(|error| self.exact_error("decode token ids", &tokenizer, error))
    }

    fn estimate_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError> {
        let tokenizer = self.tokenizer(model)?;
        let estimate = |text: &str| tokenizer.estimate_tokens(text);
        Ok(match MessageFraming::of(tokenizer.model()) {
            Some(framing) => framing.total(messages, estimate),
            None => estimate(&message_values_text(messages)),
        })
    }

    fn estimate_text(&self, model: &str, text: &str) -> Result<usize, DomainError> {
        Ok(self.tokenizer(model)?.estimate_tokens(text))
    }

    fn estimate_text_prefixes(
        &self,
        model: &str,
        base: &str,
        suffixes: &[String],
        stop_at: Option<usize>,
    ) -> Result<Vec<usize>, DomainError> {
        let tokenizer = self.tokenizer(model)?;
        let framing_tokens =
            MessageFraming::of(tokenizer.model()).map_or(0, |framing| framing.content_message());

        // Empty text has no message around it, as the frontend counts it.
        let empty_prefixes = if base.is_empty() {
            suffixes
                .iter()
                .take_while(|suffix| suffix.is_empty())
                .count()
        } else {
            0
        };
        let mut estimates = vec![0; empty_prefixes];
        if empty_prefixes == suffixes.len() || (empty_prefixes > 0 && stop_at == Some(0)) {
            estimates.resize(suffixes.len(), 0);
            return Ok(estimates);
        }

        let content_stop_at = stop_at.map(|limit| limit.saturating_sub(framing_tokens));
        estimates.extend(
            tokenizer
                .estimate_cumulative_token_counts(
                    base,
                    &suffixes[empty_prefixes..],
                    content_stop_at,
                )
                .into_iter()
                .map(|estimate| estimate + framing_tokens),
        );
        Ok(estimates)
    }
}

/// SillyTavern's count of OpenAI-style message JSON, for models whose chat format frames each
/// message: every message adds a fixed number of tokens and the tokens of each of its values, a
/// `name` key adjusts that, and the reply adds a fixed number.
struct MessageFraming {
    per_message: usize,
    per_name: isize,
    per_reply: usize,
}

impl MessageFraming {
    /// `None` when only message content counts.
    fn of(model: &Model) -> Option<Self> {
        match model.framing() {
            Framing::ContentOnly => None,
            // This snapshot counts a name in place of the role, and since 2023-10 adds up to nine
            // tokens to every request.
            Framing::ChatMl { .. } if model.id() == "gpt-3.5-turbo-0301" => Some(Self {
                per_message: 4,
                per_name: -1,
                per_reply: 3 + 9,
            }),
            Framing::ChatMl { .. } => Some(Self {
                per_message: 3,
                per_name: 1,
                per_reply: 3,
            }),
        }
    }

    fn total(&self, messages: &[Value], estimate: impl Fn(&str) -> usize) -> usize {
        let mut total = self.per_reply;
        for message in messages {
            total += self.per_message;
            match message {
                Value::Object(map) => {
                    for (key, value) in map {
                        total += estimate(&value_text(value));
                        if key == "name" {
                            total = total.saturating_add_signed(self.per_name);
                        }
                    }
                }
                _ => total += estimate(&value_text(message)),
            }
        }
        total
    }

    /// The tokens a message of only content adds to its content.
    fn content_message(&self) -> usize {
        self.per_message + self.per_reply
    }
}

fn value_text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(text) => Cow::Borrowed(text),
        _ => Cow::Owned(value.to_string()),
    }
}

/// The values of every message, as one text.
fn message_values_text(messages: &[Value]) -> String {
    let mut values = Vec::new();
    for message in messages {
        match message {
            Value::Object(map) => values.extend(map.values().map(value_text)),
            _ => values.push(value_text(message)),
        }
    }
    values.join("\n\n")
}

/// The artifact of `model`'s tokenizer, from `source` or, for a built-in encoding, from MikTik.
fn build_artifact(model: &'static Model, source: Option<Vec<u8>>) -> Result<Vec<u8>, DomainError> {
    let registry = TokenizerRegistry::new();
    if let Some(source) = source {
        registry
            .register_model_bytes(model.id(), source)
            .map_err(|error| tokenizer_error("register the tokenizer source", model.id(), error))?;
    }
    registry
        .get(model.id())
        .map(|tokenizer| tokenizer.to_bytes())
        .map_err(|error| tokenizer_error("convert the tokenizer source", model.id(), error))
}

fn load(model: &'static Model, artifact: Vec<u8>) -> Result<Arc<Tokenizer>, TokenizerError> {
    let registry = TokenizerRegistry::with_segment_cache(SEGMENT_CACHE_BYTES);
    registry.register_model_bytes(model.id(), artifact)?;
    registry.get(model.id())
}

fn decompress(
    payload: &[u8],
    compression: ResourceCompression,
    source_name: &str,
) -> Result<Vec<u8>, DomainError> {
    match compression {
        ResourceCompression::None => Ok(payload.to_vec()),
        ResourceCompression::Gzip => {
            let mut decoder = GzDecoder::new(Cursor::new(payload));
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to decompress tokenizer payload '{}': {}",
                    source_name, error
                ))
            })?;
            Ok(decompressed)
        }
    }
}

/// Removes every entry under `dir` that is not in `used` or on the way to it.
fn remove_unused(dir: &Path, used: &HashSet<PathBuf>) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if used.contains(&path) {
            continue;
        }
        if used.iter().any(|used_path| used_path.starts_with(&path)) {
            remove_unused(&path, used)?;
        } else if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Runs CPU-bound work off the async runtime.
async fn blocking<T: Send + 'static>(
    task: &'static str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, DomainError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| DomainError::InternalError(format!("{task} task failed: {error}")))
}

fn tokenizer_error(action: &str, model: &str, error: TokenizerError) -> DomainError {
    DomainError::InternalError(format!("Failed to {action} for '{model}': {error}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use miktik::Model;
    use serde_json::json;

    use super::MiktikTokenizerRepository;
    use tt_adapter_http::HttpClientPool;
    use tt_ports::repositories::tokenizer_repository::TokenizerRepository;

    const TEST_USER_AGENT: &str = "TauriTavern/test";
    static NEXT_TEMP_CACHE_DIR_ID: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_cache_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let sequence = NEXT_TEMP_CACHE_DIR_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "tauritavern-tokenizer-test-{}-{nonce}-{sequence}",
            std::process::id()
        ))
    }

    fn repository(cache_dir: &std::path::Path) -> MiktikTokenizerRepository {
        MiktikTokenizerRepository::new(
            cache_dir.to_path_buf(),
            Arc::new(HttpClientPool::new(TEST_USER_AGENT)),
        )
    }

    async fn prepared(cache_dir: &std::path::Path, models: &[&str]) -> MiktikTokenizerRepository {
        let repository = repository(cache_dir);
        for model in models {
            repository
                .ensure_model_ready(model)
                .await
                .expect("tokenizer should prepare");
        }
        repository
    }

    #[test]
    fn every_miktik_model_has_a_source() {
        for model in Model::all() {
            assert!(
                MiktikTokenizerRepository::source(model).is_some(),
                "no tokenizer source for '{}'",
                model.id()
            );
        }
    }

    #[tokio::test]
    async fn bundled_models_and_unknown_names_estimate_without_network() {
        let cache_dir = unique_temp_cache_dir();
        let names = [
            "claude-3-7-sonnet",
            "deepseek-chat",
            "gemini-2.0-flash",
            "glm-5",
            "gpt-4o",
        ];
        let repository = prepared(&cache_dir, &names).await;
        let messages = [json!({"role": "user", "content": "hello world, 世界"})];

        let estimates = names.map(|name| {
            repository
                .estimate_messages(name, &messages)
                .expect("prepared tokenizer should estimate")
        });

        let _ = std::fs::remove_dir_all(cache_dir);
        assert!(estimates.iter().all(|&estimate| estimate > 0));
        assert_eq!(estimates[3], estimates[4], "unknown names use o200k");
    }

    #[tokio::test]
    async fn local_engines_match_golden_text() {
        let cache_dir = unique_temp_cache_dir();
        let text = "Hello, 世界 👨‍👩‍👧‍👦\n";
        let cases: [(&str, &[u32]); 3] = [
            (
                "gpt2",
                &[
                    15496, 11, 220, 10310, 244, 45911, 234, 50169, 101, 447, 235, 41840, 102, 447,
                    235, 41840, 100, 447, 235, 41840, 99, 198,
                ],
            ),
            (
                "claude",
                &[
                    10002, 16, 225, 1499, 249, 37413, 41270, 244, 106, 477, 240, 53965, 107, 477,
                    240, 53965, 105, 477, 240, 53965, 104, 203,
                ],
            ),
            // Gemma 3, as the reference SentencePiece encodes it.
            (
                "gemma",
                &[
                    9259, 236764, 84679, 236743, 243874, 237243, 243767, 237243, 244814, 237243,
                    244990, 107,
                ],
            ),
        ];
        let repository = prepared(&cache_dir, &cases.map(|(model, _)| model)).await;

        for (model, expected_ids) in cases {
            let ids = repository
                .encode(model, text)
                .expect("golden text should encode");
            assert_eq!(ids, expected_ids, "token ids changed for '{model}'");
            assert_eq!(
                repository
                    .decode(model, &ids)
                    .expect("golden token ids should decode"),
                text,
                "roundtrip changed for '{model}'"
            );
        }

        let _ = std::fs::remove_dir_all(cache_dir);
    }

    #[tokio::test]
    async fn tokenizers_load_from_artifacts_and_the_cache_keeps_only_what_is_used() {
        let cache_dir = unique_temp_cache_dir();
        let text = "Artifacts load in milliseconds.\n世界设定 🙂\n";
        let models = ["gpt-4o", "claude"];
        let compiled = prepared(&cache_dir, &models).await;
        let estimates = models.map(|model| compiled.estimate_text(model, text).unwrap());
        let ids = models.map(|model| compiled.encode(model, text).unwrap());

        let artifact_dir = cache_dir
            .join("compiled")
            .join(format!("v{}", miktik::ARTIFACT_VERSION));
        let claude_artifact = artifact_dir.join("claude-claude.json.mtk");
        let stale = [
            cache_dir
                .join("compiled")
                .join("v0")
                .join("claude-claude.json.mtk"),
            artifact_dir.join("gemma-gemma.model.mtk"),
            cache_dir.join("gemma.model"),
            cache_dir.join(".claude.json.1.2.tmp"),
        ];
        for path in &stale {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"stale").unwrap();
        }
        std::fs::write(&claude_artifact, b"not an artifact").unwrap();

        let loaded = prepared(&cache_dir, &models).await;
        let reloaded_estimates = models.map(|model| loaded.estimate_text(model, text).unwrap());
        let reloaded_ids = models.map(|model| loaded.encode(model, text).unwrap());
        let rebuilt = std::fs::read(&claude_artifact).unwrap();
        let mut kept = std::fs::read_dir(&artifact_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        kept.sort();
        let stale_left = stale.iter().filter(|path| path.exists()).count();

        let _ = std::fs::remove_dir_all(cache_dir);
        assert_eq!(reloaded_estimates, estimates);
        assert_eq!(reloaded_ids, ids);
        assert!(
            rebuilt.starts_with(b"MIKTIK"),
            "the damaged artifact is rebuilt"
        );
        assert_eq!(kept, ["claude-claude.json.mtk", "gpt-4o-o200k_base.mtk"]);
        assert_eq!(stale_left, 0);
    }

    #[tokio::test]
    async fn openai_0301_keeps_legacy_message_count_rules() {
        let cache_dir = unique_temp_cache_dir();
        let repository = prepared(&cache_dir, &["gpt-3.5-turbo-0301", "gpt-4"]).await;
        let messages = [json!({"role": "user", "name": "Alice", "content": "hello"})];

        let legacy = repository
            .estimate_messages("gpt-3.5-turbo-0301", &messages)
            .unwrap();
        let modern = repository.estimate_messages("gpt-4", &messages).unwrap();

        let _ = std::fs::remove_dir_all(cache_dir);
        assert_eq!(legacy, modern + 8);
    }

    #[tokio::test]
    async fn prefix_estimates_equal_the_estimate_of_each_prefix() {
        let cache_dir = unique_temp_cache_dir();
        let models = [
            "gpt-4o",
            "gpt-3.5-turbo-0301",
            "claude",
            "deepseek",
            "gemma",
        ];
        let repository = prepared(&cache_dir, &models).await;
        let fragments = [
            "",
            "a",
            "bc",
            "  ",
            "\n",
            " \n ",
            "punctuation!?",
            "世界",
            "设定。",
            "\u{1f642}",
            "e\u{301}",
            "<|not-a-special-token|>",
        ];

        for model in models {
            for base_index in 0..fragments.len() {
                let base = fragments[..base_index].concat();
                let suffixes = (0..32)
                    .map(|index| fragments[(base_index + index) % fragments.len()].to_string())
                    .collect::<Vec<_>>();

                let mut content = base.clone();
                let expected = suffixes
                    .iter()
                    .map(|suffix| {
                        content.push_str(suffix);
                        if content.is_empty() {
                            return 0;
                        }
                        repository
                            .estimate_messages(model, &[json!({ "content": content })])
                            .unwrap()
                    })
                    .collect::<Vec<_>>();
                let actual = repository
                    .estimate_text_prefixes(model, &base, &suffixes, None)
                    .unwrap();
                assert_eq!(actual, expected, "prefix estimates changed for {model}");

                let stop_at = expected[20];
                let stop_index = expected.iter().position(|&count| count >= stop_at).unwrap();
                let mut stopped = expected.clone();
                stopped[stop_index..].fill(expected[stop_index]);
                assert_eq!(
                    repository
                        .estimate_text_prefixes(model, &base, &suffixes, Some(stop_at))
                        .unwrap(),
                    stopped,
                    "prefix estimates did not stop for {model}"
                );
            }
        }

        let _ = std::fs::remove_dir_all(cache_dir);
    }
}
