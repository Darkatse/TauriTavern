use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use tt_domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct SdRouteRequest {
    pub path: String,
    pub body: Value,
    pub credentials: SdRouteCredentials,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdRouteCredentials {
    None,
    WorkersAi { api_key: String },
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
