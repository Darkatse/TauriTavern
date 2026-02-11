use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiTokenCountRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub messages: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiTokenCountResponseDto {
    pub token_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiEncodeRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiEncodeResponseDto {
    pub ids: Vec<u32>,
    pub count: usize,
    pub chunks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiDecodeRequestDto {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiDecodeResponseDto {
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
