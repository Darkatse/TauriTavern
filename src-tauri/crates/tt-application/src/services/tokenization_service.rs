use std::collections::HashMap;
use std::sync::Arc;

use crate::dto::tokenization_dto::{
    DecodeTokensRequestDto, DecodeTokensResponseDto, EncodeTokensRequestDto,
    EncodeTokensResponseDto, LogitBiasEntryDto, OpenAiLogitBiasRequestDto,
    OpenAiLogitBiasResponseDto, OpenAiTokenCountBatchRequestDto, OpenAiTokenCountBatchResponseDto,
    OpenAiTokenPrefixCountRequestDto, TextTokenCountRequestDto, TextTokenCountResponseDto,
};
use crate::errors::ApplicationError;
use serde_json::Value;
use tt_domain::errors::DomainError;
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;

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
        let OpenAiTokenCountBatchRequestDto { model, requests } = dto;
        self.tokenizer_repository.ensure_model_ready(&model).await?;

        let token_counts = self
            .blocking("Token count batch", move |repository| {
                let requests: Vec<&[Value]> = requests
                    .iter()
                    .map(|request| request.messages.as_slice())
                    .collect();
                repository.estimate_messages(&model, &requests)
            })
            .await?;

        Ok(OpenAiTokenCountBatchResponseDto { token_counts })
    }

    pub async fn count_openai_token_prefixes(
        &self,
        dto: OpenAiTokenPrefixCountRequestDto,
    ) -> Result<OpenAiTokenCountBatchResponseDto, ApplicationError> {
        let OpenAiTokenPrefixCountRequestDto {
            model,
            base,
            suffixes,
            stop_at,
        } = dto;
        self.tokenizer_repository.ensure_model_ready(&model).await?;

        let token_counts = self
            .blocking("Token prefix count", move |repository| {
                repository.estimate_text_prefixes(&model, &base, &suffixes, stop_at)
            })
            .await?;

        Ok(OpenAiTokenCountBatchResponseDto { token_counts })
    }

    pub async fn count_text_tokens(
        &self,
        dto: TextTokenCountRequestDto,
    ) -> Result<TextTokenCountResponseDto, ApplicationError> {
        let TextTokenCountRequestDto { model, text } = dto;
        self.tokenizer_repository.ensure_model_ready(&model).await?;

        let count = self
            .blocking("Text token count", move |repository| {
                repository.estimate_text(&model, &text)
            })
            .await?;

        Ok(TextTokenCountResponseDto { count })
    }

    pub async fn encode_tokens(
        &self,
        dto: EncodeTokensRequestDto,
    ) -> Result<EncodeTokensResponseDto, ApplicationError> {
        let EncodeTokensRequestDto { model, text } = dto;
        self.tokenizer_repository.ensure_model_ready(&model).await?;

        self.blocking("Token encode", move |repository| {
            let ids = repository.encode(&model, &text)?;
            let chunks = decode_token_chunks(repository, &model, &ids);
            Ok(EncodeTokensResponseDto {
                count: ids.len(),
                ids,
                chunks,
            })
        })
        .await
    }

    pub async fn decode_tokens(
        &self,
        dto: DecodeTokensRequestDto,
    ) -> Result<DecodeTokensResponseDto, ApplicationError> {
        let DecodeTokensRequestDto { model, ids } = dto;
        self.tokenizer_repository.ensure_model_ready(&model).await?;

        self.blocking("Token decode", move |repository| {
            let text = repository.decode(&model, &ids)?;
            let chunks = decode_token_chunks(repository, &model, &ids);
            Ok(DecodeTokensResponseDto { text, chunks })
        })
        .await
    }

    pub async fn build_openai_logit_bias(
        &self,
        dto: OpenAiLogitBiasRequestDto,
    ) -> Result<OpenAiLogitBiasResponseDto, ApplicationError> {
        let OpenAiLogitBiasRequestDto { model, entries } = dto;
        self.tokenizer_repository.ensure_model_ready(&model).await?;

        self.blocking("Logit bias", move |repository| {
            let mut bias: HashMap<String, f32> = HashMap::new();
            for (entry_index, entry) in entries.into_iter().enumerate() {
                let token_ids = match entry_tokens(repository, &model, &entry) {
                    Ok(token_ids) => token_ids,
                    Err(error) => {
                        tracing::warn!(
                            model,
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
        })
        .await
    }

    /// Runs tokenizer work off the async runtime: long texts take milliseconds, and the first
    /// exact call of a tokenizer builds its exact engine.
    async fn blocking<T: Send + 'static>(
        &self,
        task: &'static str,
        work: impl FnOnce(&dyn TokenizerRepository) -> Result<T, DomainError> + Send + 'static,
    ) -> Result<T, ApplicationError> {
        let repository = Arc::clone(&self.tokenizer_repository);
        tokio::task::spawn_blocking(move || work(repository.as_ref()))
            .await
            .map_err(|error| {
                ApplicationError::InternalError(format!("{task} task failed: {error}"))
            })?
            .map_err(ApplicationError::from)
    }
}

fn entry_tokens(
    repository: &dyn TokenizerRepository,
    model: &str,
    entry: &LogitBiasEntryDto,
) -> Result<Vec<u32>, DomainError> {
    if let Some(ids) = parse_inline_token_ids(&entry.text) {
        return Ok(ids);
    }

    repository.encode(model, &entry.text)
}

fn decode_token_chunks(
    repository: &dyn TokenizerRepository,
    model: &str,
    ids: &[u32],
) -> Vec<String> {
    ids.iter()
        .map(|id| {
            // A token may contain only part of a UTF-8 sequence. Chunks are
            // display metadata, so decode them lossily without hiding a full decode error.
            repository
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

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

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

        fn estimate_messages(
            &self,
            _model: &str,
            _requests: &[&[Value]],
        ) -> Result<Vec<usize>, DomainError> {
            unreachable!("token chunk tests do not count messages")
        }

        fn estimate_text(&self, _model: &str, _text: &str) -> Result<usize, DomainError> {
            unreachable!("token chunk tests do not count text")
        }

        fn estimate_text_prefixes(
            &self,
            _model: &str,
            _base: &str,
            _suffixes: &[String],
            _stop_at: Option<usize>,
        ) -> Result<Vec<usize>, DomainError> {
            unreachable!("token chunk tests do not count prefixes")
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
