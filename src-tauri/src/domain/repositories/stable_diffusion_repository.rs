use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct SdRouteRequest {
    pub path: String,
    pub body: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdRouteResponseKind {
    Json,
    Text,
    Empty,
}

#[derive(Debug, Clone)]
pub struct SdRouteResponse {
    pub status: u16,
    pub kind: SdRouteResponseKind,
    pub body: Value,
}

#[async_trait]
pub trait StableDiffusionRepository: Send + Sync {
    async fn handle(
        &self,
        request: SdRouteRequest,
        cancel: watch::Receiver<bool>,
    ) -> Result<SdRouteResponse, DomainError>;
}
