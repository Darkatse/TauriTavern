use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiTokenCountBatchItemDto {
    #[serde(default)]
    pub messages: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiTokenCountBatchRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub requests: Vec<OpenAiTokenCountBatchItemDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiTokenCountBatchResponseDto {
    pub token_counts: Vec<usize>,
    /// The fallback tokenizer counted in place of the model's own, which could not be downloaded:
    /// a later count may differ.
    pub fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiTokenPrefixCountRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub suffixes: Vec<String>,
    /// Threshold in caller-visible text token counts, matching the returned counts.
    #[serde(default)]
    pub stop_at: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextTokenCountRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextTokenCountResponseDto {
    pub count: usize,
    /// See [`OpenAiTokenCountBatchResponseDto::fallback`].
    pub fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EncodeTokensRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EncodeTokensResponseDto {
    pub ids: Vec<u32>,
    pub count: usize,
    pub chunks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecodeTokensRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecodeTokensResponseDto {
    pub text: String,
    pub chunks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogitBiasEntryDto {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiLogitBiasRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub entries: Vec<LogitBiasEntryDto>,
}

pub type OpenAiLogitBiasResponseDto = HashMap<String, f32>;
