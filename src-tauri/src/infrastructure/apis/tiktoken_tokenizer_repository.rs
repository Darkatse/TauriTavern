use std::collections::HashMap;
use std::sync::RwLock;

use serde_json::Value;
use tiktoken_rs::tokenizer::get_tokenizer;
use tiktoken_rs::{get_bpe_from_model, CoreBPE};

use crate::domain::errors::DomainError;
use crate::domain::repositories::tokenizer_repository::TokenizerRepository;

const DEFAULT_FALLBACK_MODEL: &str = "gpt-4o";

pub struct TiktokenTokenizerRepository {
    cache: RwLock<HashMap<String, CoreBPE>>,
    fallback_model: &'static str,
}

impl TiktokenTokenizerRepository {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            fallback_model: DEFAULT_FALLBACK_MODEL,
        }
    }

    fn resolve_tokenizer_model<'a>(&'a self, requested_model: &'a str) -> &'a str {
        let trimmed = requested_model.trim();

        if trimmed.is_empty() {
            return self.fallback_model;
        }

        if get_tokenizer(trimmed).is_some() {
            trimmed
        } else {
            self.fallback_model
        }
    }

    fn get_bpe(&self, requested_model: &str) -> Result<CoreBPE, DomainError> {
        let model = self.resolve_tokenizer_model(requested_model).to_string();

        if let Some(cached) = self
            .cache
            .read()
            .map_err(|error| {
                DomainError::InternalError(format!("Tokenizer cache read lock failed: {error}"))
            })?
            .get(&model)
            .cloned()
        {
            return Ok(cached);
        }

        let bpe = get_bpe_from_model(&model)
            .or_else(|_| get_bpe_from_model(self.fallback_model))
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to initialize tokenizer for model '{}': {}",
                    requested_model, error
                ))
            })?;

        self.cache
            .write()
            .map_err(|error| {
                DomainError::InternalError(format!("Tokenizer cache write lock failed: {error}"))
            })?
            .insert(model, bpe.clone());

        Ok(bpe)
    }

    fn value_to_text(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            _ => value.to_string(),
        }
    }

    fn is_gpt_3_5_0301(model: &str) -> bool {
        model.contains("gpt-3.5-turbo-0301")
    }
}

impl Default for TiktokenTokenizerRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenizerRepository for TiktokenTokenizerRepository {
    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError> {
        let bpe = self.get_bpe(model)?;
        Ok(bpe.encode_with_special_tokens(text))
    }

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError> {
        let bpe = self.get_bpe(model)?;
        bpe.decode(token_ids.to_vec()).map_err(|error| {
            DomainError::InternalError(format!("Failed to decode token ids: {error}"))
        })
    }

    fn count_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError> {
        let bpe = self.get_bpe(model)?;
        let is_legacy = Self::is_gpt_3_5_0301(model);
        let tokens_per_message = if is_legacy { 4_i32 } else { 3_i32 };
        let tokens_per_name = if is_legacy { -1_i32 } else { 1_i32 };
        let mut total = 0_i32;

        for message in messages {
            total += tokens_per_message;

            match message {
                Value::Object(map) => {
                    for (key, value) in map {
                        let text = Self::value_to_text(value);
                        total += bpe.encode_with_special_tokens(&text).len() as i32;
                        if key == "name" {
                            total += tokens_per_name;
                        }
                    }
                }
                _ => {
                    let text = Self::value_to_text(message);
                    total += bpe.encode_with_special_tokens(&text).len() as i32;
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
