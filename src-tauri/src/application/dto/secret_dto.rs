use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretDto {
    pub key: String,
    pub value: String,
}

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
