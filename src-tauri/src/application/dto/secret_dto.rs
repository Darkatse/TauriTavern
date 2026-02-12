use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretStateItemDto {
    pub id: String,
    pub value: String,
    pub label: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecretStateDto {
    #[serde(flatten)]
    pub states: HashMap<String, Option<Vec<SecretStateItemDto>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AllSecretsDto {
    #[serde(flatten)]
    pub secrets: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSecretDto {
    pub key: String,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSecretResponseDto {
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteSecretDto {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSecretDto {
    pub key: String,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateSecretDto {
    pub key: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameSecretDto {
    pub key: String,
    pub id: String,
    pub label: String,
}
