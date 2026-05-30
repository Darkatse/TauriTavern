use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{RwLock, watch};

use crate::application::dto::stable_diffusion_dto::{SdRouteResponseDto, SdRouteResponseKindDto};
use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::stable_diffusion_repository::{
    SdRouteCredentials, SdRouteRequest, SdRouteResponseKind, StableDiffusionRepository,
};

pub struct StableDiffusionService {
    repository: Arc<dyn StableDiffusionRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    active_requests: CancellationRegistry,
}

impl StableDiffusionService {
    pub fn new(
        repository: Arc<dyn StableDiffusionRepository>,
        secret_repository: Arc<dyn SecretRepository>,
    ) -> Self {
        Self {
            repository,
            secret_repository,
            active_requests: CancellationRegistry::default(),
        }
    }

    pub async fn handle_request(
        &self,
        request_id: &str,
        path: String,
        body: Value,
    ) -> Result<SdRouteResponseDto, ApplicationError> {
        let path = path.trim().trim_matches('/').to_ascii_lowercase();
        let credentials = if matches!(path.as_str(), "workersai/models" | "workersai/generate") {
            let Some(api_key) = self
                .secret_repository
                .read_secret(SecretKeys::WORKERS_AI, None)
                .await?
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            else {
                return Ok(text_response(
                    400,
                    "Cloudflare Workers AI API key is required",
                ));
            };
            SdRouteCredentials::WorkersAi { api_key }
        } else {
            SdRouteCredentials::None
        };

        let cancel = self.active_requests.register(request_id).await;
        let result = self
            .repository
            .handle(
                SdRouteRequest {
                    path,
                    body,
                    credentials,
                },
                cancel,
            )
            .await;
        self.active_requests.complete(request_id).await;

        let response = result.map_err(ApplicationError::from)?;

        Ok(SdRouteResponseDto {
            status: response.status,
            kind: match response.kind {
                SdRouteResponseKind::Json => SdRouteResponseKindDto::Json,
                SdRouteResponseKind::Text => SdRouteResponseKindDto::Text,
                SdRouteResponseKind::Empty => SdRouteResponseKindDto::Empty,
            },
            body: response.body,
        })
    }

    pub async fn cancel_request(&self, request_id: &str) -> bool {
        self.active_requests.cancel(request_id).await
    }
}

fn text_response(status: u16, message: impl Into<String>) -> SdRouteResponseDto {
    SdRouteResponseDto {
        status,
        kind: SdRouteResponseKindDto::Text,
        body: Value::String(message.into()),
    }
}

#[derive(Default)]
struct CancellationRegistry {
    active: RwLock<HashMap<String, watch::Sender<bool>>>,
}

impl CancellationRegistry {
    async fn register(&self, request_id: &str) -> watch::Receiver<bool> {
        let (sender, receiver) = watch::channel(false);
        let mut active = self.active.write().await;

        if let Some(previous_sender) = active.insert(request_id.to_string(), sender) {
            let _ = previous_sender.send(true);
        }

        receiver
    }

    async fn cancel(&self, request_id: &str) -> bool {
        let mut active = self.active.write().await;
        let Some(sender) = active.remove(request_id) else {
            return false;
        };

        let _ = sender.send(true);
        true
    }

    async fn complete(&self, request_id: &str) {
        let mut active = self.active.write().await;
        active.remove(request_id);
    }
}
