use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{RwLock, watch};
use url::Url;

use crate::dto::stable_diffusion_dto::{SdRouteResponseDto, SdRouteResponseKindDto};
use crate::errors::ApplicationError;
use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::stable_diffusion_repository::{
    SdRouteCredentials, SdRouteRequest, SdRouteResponseKind, StableDiffusionRepository,
};

const COMFY_WORKFLOW_ROUTES: [&str; 5] = [
    "comfy/workflows",
    "comfy/workflow",
    "comfy/save-workflow",
    "comfy/delete-workflow",
    "comfy/rename-workflow",
];

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

    pub fn resolve_user_endpoint(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<Option<Url>, ApplicationError> {
        let path = normalize_path(path);
        if is_workers_ai_route(&path) || COMFY_WORKFLOW_ROUTES.contains(&path.as_str()) {
            return Ok(None);
        }
        let Some(url) = body
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            return Ok(None);
        };
        Url::parse(url).map(Some).map_err(|error| {
            ApplicationError::ValidationError(format!("Invalid server URL {url:?}: {error}"))
        })
    }

    pub async fn handle_request(
        &self,
        request_id: &str,
        path: String,
        body: Value,
    ) -> Result<SdRouteResponseDto, ApplicationError> {
        let path = normalize_path(&path);
        let credentials = if is_workers_ai_route(&path) {
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

fn normalize_path(path: &str) -> String {
    path.trim().trim_matches('/').to_ascii_lowercase()
}

fn is_workers_ai_route(path: &str) -> bool {
    matches!(path, "workersai/models" | "workersai/generate")
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
