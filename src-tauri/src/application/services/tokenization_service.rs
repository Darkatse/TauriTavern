use std::collections::HashMap;
use std::sync::Arc;

use crate::application::dto::tokenization_dto::{
    LogitBiasEntryDto, OpenAiDecodeRequestDto, OpenAiDecodeResponseDto, OpenAiEncodeRequestDto,
    OpenAiEncodeResponseDto, OpenAiLogitBiasRequestDto, OpenAiLogitBiasResponseDto,
    OpenAiTokenCountRequestDto, OpenAiTokenCountResponseDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::repositories::tokenizer_repository::TokenizerRepository;

const DEFAULT_MODEL: &str = "gpt-4o";

pub struct TokenizationService {
    tokenizer_repository: Arc<dyn TokenizerRepository>,
}

impl TokenizationService {
    pub fn new(tokenizer_repository: Arc<dyn TokenizerRepository>) -> Self {
        Self {
            tokenizer_repository,
        }
    }

    pub fn count_openai_tokens(
        &self,
        dto: OpenAiTokenCountRequestDto,
    ) -> Result<OpenAiTokenCountResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        let token_count = self
            .tokenizer_repository
            .count_messages(&model, &dto.messages)?;

        Ok(OpenAiTokenCountResponseDto { token_count })
    }

    pub fn encode_openai_tokens(
        &self,
        dto: OpenAiEncodeRequestDto,
    ) -> Result<OpenAiEncodeResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        let ids = self.tokenizer_repository.encode(&model, &dto.text)?;

        let chunks = ids
            .iter()
            .map(|id| {
                self.tokenizer_repository
                    .decode(&model, &[*id])
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();

        Ok(OpenAiEncodeResponseDto {
            count: ids.len(),
            ids,
            chunks,
        })
    }

    pub fn decode_openai_tokens(
        &self,
        dto: OpenAiDecodeRequestDto,
    ) -> Result<OpenAiDecodeResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        let text = self.tokenizer_repository.decode(&model, &dto.ids)?;
        let chunks = dto
            .ids
            .iter()
            .map(|id| {
                self.tokenizer_repository
                    .decode(&model, &[*id])
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();

        Ok(OpenAiDecodeResponseDto { text, chunks })
    }

    pub fn build_openai_logit_bias(
        &self,
        dto: OpenAiLogitBiasRequestDto,
    ) -> Result<OpenAiLogitBiasResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        let mut bias: HashMap<String, f32> = HashMap::new();

        for entry in dto.entries {
            for token_id in self.resolve_entry_tokens(&model, &entry)? {
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

    fn normalize_model(&self, model: &str) -> String {
        let normalized = model.trim();
        if normalized.is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            normalized.to_string()
        }
    }
}
