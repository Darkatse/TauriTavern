use std::borrow::Cow;
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

    pub async fn count_openai_tokens(
        &self,
        dto: OpenAiTokenCountRequestDto,
    ) -> Result<OpenAiTokenCountResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;
        let token_count = self
            .tokenizer_repository
            .count_messages(model.as_ref(), &dto.messages)?;

        Ok(OpenAiTokenCountResponseDto { token_count })
    }

    pub async fn encode_openai_tokens(
        &self,
        dto: OpenAiEncodeRequestDto,
    ) -> Result<OpenAiEncodeResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;
        let ids = self
            .tokenizer_repository
            .encode(model.as_ref(), &dto.text)?;

        let mut chunks = Vec::with_capacity(ids.len());
        for id in &ids {
            chunks.push(self.tokenizer_repository.decode(model.as_ref(), &[*id])?);
        }

        Ok(OpenAiEncodeResponseDto {
            count: ids.len(),
            ids,
            chunks,
        })
    }

    pub async fn decode_openai_tokens(
        &self,
        dto: OpenAiDecodeRequestDto,
    ) -> Result<OpenAiDecodeResponseDto, ApplicationError> {
        let model = self.normalize_model(&dto.model);
        self.tokenizer_repository
            .ensure_model_ready(model.as_ref())
            .await?;
        let text = self.tokenizer_repository.decode(model.as_ref(), &dto.ids)?;

        let mut chunks = Vec::with_capacity(dto.ids.len());
        for id in &dto.ids {
            chunks.push(self.tokenizer_repository.decode(model.as_ref(), &[*id])?);
        }

        Ok(OpenAiDecodeResponseDto { text, chunks })
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

        for entry in dto.entries {
            for token_id in self.resolve_entry_tokens(model.as_ref(), &entry)? {
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

    fn normalize_model<'a>(&self, model: &'a str) -> Cow<'a, str> {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            Cow::Borrowed(DEFAULT_MODEL)
        } else {
            Cow::Borrowed(trimmed)
        }
    }
}
