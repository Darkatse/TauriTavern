use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::dto::tokenization_dto::{
    DecodeTokensRequestDto, DecodeTokensResponseDto, EncodeTokensRequestDto,
    EncodeTokensResponseDto, LogitBiasEntryDto, OpenAiLogitBiasRequestDto,
    OpenAiLogitBiasResponseDto, OpenAiTokenCountBatchRequestDto, OpenAiTokenCountBatchResponseDto,
    OpenAiTokenPrefixCountRequestDto,
};
use crate::errors::ApplicationError;
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;

const DEFAULT_MODEL: &str = "gpt-4o";
const REPLACEMENT_CHARACTER: &str = "\u{fffd}";

pub struct TokenizationService {
    tokenizer_repository: Arc<dyn TokenizerRepository>,
}

impl TokenizationService {
    pub fn new(tokenizer_repository: Arc<dyn TokenizerRepository>) -> Self {
        Self {
            tokenizer_repository,
        }
    }

    pub async fn count_openai_tokens_batch(
        &self,
        dto: OpenAiTokenCountBatchRequestDto,
    ) -> Result<OpenAiTokenCountBatchResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;

        let tokenizer_repository = Arc::clone(&self.tokenizer_repository);
        let model = model.into_owned();
        let requests = dto.requests;

        let token_counts = tokio::task::spawn_blocking(move || {
            let mut token_counts = Vec::with_capacity(requests.len());

            for request in requests {
                let token_count = tokenizer_repository
                    .count_messages(&model, &request.messages)
                    .map_err(ApplicationError::from)?;
                token_counts.push(token_count);
            }

            Ok::<_, ApplicationError>(token_counts)
        })
        .await
        .map_err(|error| {
            ApplicationError::InternalError(format!("Token count batch task failed: {error}"))
        })??;

        Ok(OpenAiTokenCountBatchResponseDto { token_counts })
    }

    pub async fn count_openai_token_prefixes(
        &self,
        dto: OpenAiTokenPrefixCountRequestDto,
    ) -> Result<OpenAiTokenCountBatchResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;

        let tokenizer_repository = Arc::clone(&self.tokenizer_repository);
        let model = model.into_owned();
        let base = dto.base;
        let suffixes = dto.suffixes;
        let stop_at = dto.stop_at;

        let token_counts = tokio::task::spawn_blocking(move || {
            tokenizer_repository
                .count_system_message_prefixes(&model, &base, &suffixes, stop_at)
                .map_err(ApplicationError::from)
        })
        .await
        .map_err(|error| {
            ApplicationError::InternalError(format!("Token prefix count task failed: {error}"))
        })??;

        Ok(OpenAiTokenCountBatchResponseDto { token_counts })
    }

    pub async fn encode_tokens(
        &self,
        dto: EncodeTokensRequestDto,
    ) -> Result<EncodeTokensResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;
        let ids = self
            .tokenizer_repository
            .encode(model.as_ref(), &dto.text)?;

        let chunks = self.decode_token_chunks(model.as_ref(), &ids);

        Ok(EncodeTokensResponseDto {
            count: ids.len(),
            ids,
            chunks,
        })
    }

    pub async fn decode_tokens(
        &self,
        dto: DecodeTokensRequestDto,
    ) -> Result<DecodeTokensResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;
        let text = self.tokenizer_repository.decode(model.as_ref(), &dto.ids)?;

        let chunks = self.decode_token_chunks(model.as_ref(), &dto.ids);

        Ok(DecodeTokensResponseDto { text, chunks })
    }

    pub async fn build_openai_logit_bias(
        &self,
        dto: OpenAiLogitBiasRequestDto,
    ) -> Result<OpenAiLogitBiasResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;
        let mut bias: HashMap<String, f32> = HashMap::new();

        for (entry_index, entry) in dto.entries.into_iter().enumerate() {
            let token_ids = match self.resolve_entry_tokens(model.as_ref(), &entry) {
                Ok(token_ids) => token_ids,
                Err(error) => {
                    tracing::warn!(
                        model = model.as_ref(),
                        entry_index,
                        %error,
                        "Skipping logit bias entry that could not be encoded"
                    );
                    continue;
                }
            };

            for token_id in token_ids {
                bias.insert(token_id.to_string(), entry.value);
            }
        }

        Ok(bias)
    }

    fn resolve_entry_tokens(
        &self,
        model: &str,
        entry: &LogitBiasEntryDto,
    ) -> Result<Vec<u32>, ApplicationError> {
        if let Some(ids) = Self::parse_inline_token_ids(&entry.text) {
            return Ok(ids);
        }

        self.tokenizer_repository
            .encode(model, &entry.text)
            .map_err(ApplicationError::from)
    }

    fn decode_token_chunks(&self, model: &str, ids: &[u32]) -> Vec<String> {
        ids.iter()
            .map(|id| {
                // A token may contain only part of a UTF-8 sequence. Chunks are
                // display metadata, so decode them lossily without hiding a full decode error.
                self.tokenizer_repository
                    .decode(model, &[*id])
                    .unwrap_or_else(|_| REPLACEMENT_CHARACTER.to_string())
            })
            .collect()
    }

    fn parse_inline_token_ids(text: &str) -> Option<Vec<u32>> {
        let trimmed = text.trim();

        if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
            return None;
        }

        let value = serde_json::from_str::<serde_json::Value>(trimmed).ok()?;
        let array = value.as_array()?;
        let mut ids = Vec::with_capacity(array.len());

        for item in array {
            let value = item.as_u64()?;
            if value > u32::MAX as u64 {
                return None;
            }
            ids.push(value as u32);
        }

        Some(ids)
    }

    fn normalize_model<'a>(&self, model: &'a str) -> Cow<'a, str> {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            Cow::Borrowed(DEFAULT_MODEL)
        } else {
            Cow::Borrowed(trimmed)
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::Value;
    use tt_domain::errors::DomainError;

    use super::*;

    struct FragmentingTokenizerRepository;

    #[async_trait]
    impl TokenizerRepository for FragmentingTokenizerRepository {
        async fn ensure_model_ready(&self, _model: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn encode(&self, _model: &str, text: &str) -> Result<Vec<u32>, DomainError> {
            if text == "bad" {
                return Err(DomainError::InvalidData(
                    "logit bias entry cannot be encoded".to_string(),
                ));
            }
            Ok(vec![1, 2])
        }

        fn decode(&self, _model: &str, token_ids: &[u32]) -> Result<String, DomainError> {
            match token_ids {
                [1] => Err(DomainError::InternalError(
                    "token is an incomplete UTF-8 fragment".to_string(),
                )),
                [2] => Ok("B".to_string()),
                [1, 2] => Ok("AB".to_string()),
                _ => Err(DomainError::InvalidData("invalid token ids".to_string())),
            }
        }

        fn count_messages(&self, _model: &str, _messages: &[Value]) -> Result<usize, DomainError> {
            unreachable!("token chunk tests do not count messages")
        }
    }

    #[tokio::test]
    async fn token_chunks_are_lossy_but_full_decode_stays_strict() {
        let service = TokenizationService::new(Arc::new(FragmentingTokenizerRepository));

        let encoded = service
            .encode_tokens(EncodeTokensRequestDto {
                model: "gpt2".to_string(),
                text: "ignored".to_string(),
            })
            .await
            .expect("encoding should preserve token ids when a display chunk is partial");
        assert_eq!(encoded.ids, vec![1, 2]);
        assert_eq!(encoded.chunks, vec![REPLACEMENT_CHARACTER, "B"]);

        let decoded = service
            .decode_tokens(DecodeTokensRequestDto {
                model: "gpt2".to_string(),
                ids: vec![1, 2],
            })
            .await
            .expect("the complete token sequence should decode");
        assert_eq!(decoded.text, "AB");
        assert_eq!(decoded.chunks, vec![REPLACEMENT_CHARACTER, "B"]);

        assert!(
            service
                .decode_tokens(DecodeTokensRequestDto {
                    model: "gpt2".to_string(),
                    ids: vec![3],
                })
                .await
                .is_err(),
            "a full decode error must not be hidden by lossy chunk rendering"
        );
    }

    #[tokio::test]
    async fn logit_bias_skips_only_the_entry_that_cannot_be_encoded() {
        let service = TokenizationService::new(Arc::new(FragmentingTokenizerRepository));

        let bias = service
            .build_openai_logit_bias(OpenAiLogitBiasRequestDto {
                model: "gpt2".to_string(),
                entries: vec![
                    LogitBiasEntryDto {
                        text: "bad".to_string(),
                        value: -1.0,
                    },
                    LogitBiasEntryDto {
                        text: "good".to_string(),
                        value: 0.75,
                    },
                ],
            })
            .await
            .expect("one invalid entry should not discard valid logit bias entries");

        assert_eq!(bias.get("1"), Some(&0.75));
        assert_eq!(bias.get("2"), Some(&0.75));
    }
}
