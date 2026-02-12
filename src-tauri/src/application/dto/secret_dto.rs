use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretStateDto {
    #[serde(flatten)]
    pub states: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllSecretsDto {
    #[serde(flatten)]
    pub secrets: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSecretDto {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSecretResponseDto {
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteSecretDto {
    pub key: String,
    pub value: String,
}
