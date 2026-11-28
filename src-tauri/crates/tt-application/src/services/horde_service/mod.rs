use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::sync::Mutex;
use tokio::time::Instant;

use tt_domain::ios_policy::{IosPolicyActivationReport, IosPolicyScope};
use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::horde_repository::HordeRepository;
use tt_ports::repositories::secret_repository::SecretRepository;

use crate::dto::horde_dto::{HordeTextGenerationRequestDto, HordeUserInfoDto};
use crate::errors::ApplicationError;
use crate::services::generation_runtime::{GenerationRuntime, is_quiet};
use crate::services::selected_secret::read_selected_secret;

mod text;

const ANONYMOUS_API_KEY: &str = "0000000000";
const LIST_CACHE_TTL: Duration = Duration::from_secs(60);

pub struct HordeService {
    repository: Arc<dyn HordeRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    generation_runtime: Arc<GenerationRuntime>,
    ios_policy: IosPolicyActivationReport,
    text_workers: CachedList,
    text_models: CachedList,
}

impl HordeService {
    pub fn new(
        repository: Arc<dyn HordeRepository>,
        secret_repository: Arc<dyn SecretRepository>,
        generation_runtime: Arc<GenerationRuntime>,
        ios_policy: IosPolicyActivationReport,
    ) -> Self {
        Self {
            repository,
            secret_repository,
            generation_runtime,
            ios_policy,
            text_workers: CachedList::default(),
            text_models: CachedList::default(),
        }
    }

    pub async fn check_status(&self) -> Result<(), ApplicationError> {
        Ok(self.horde()?.heartbeat().await?)
    }

    pub async fn text_workers(&self, force: bool) -> Result<Value, ApplicationError> {
        let horde = self.horde()?;
        self.text_workers
            .get_or_fetch(force, || async { Ok(horde.text_workers().await?) })
            .await
    }

    pub async fn text_models(&self, force: bool) -> Result<Value, ApplicationError> {
        let horde = self.horde()?;
        self.text_models
            .get_or_fetch(force, || async {
                let (models, reference) =
                    tokio::join!(horde.text_models(), horde.text_model_reference());
                let Value::Array(models) = models? else {
                    return Err(ApplicationError::InternalError(
                        "The Horde text model list is not an array".to_string(),
                    ));
                };

                let models = match reference {
                    Ok(Value::Object(reference)) => with_reference(models, &reference),
                    Ok(_) => {
                        tracing::warn!("The Horde text model reference is not an object");
                        models
                    }
                    Err(error) => {
                        tracing::warn!("Failed to read the Horde text model reference: {error}");
                        models
                    }
                };
                Ok(Value::Array(models))
            })
            .await
    }

    pub async fn user_info(&self) -> Result<HordeUserInfoDto, ApplicationError> {
        let horde = self.horde()?;
        let Some(api_key) = self.api_key().await? else {
            return Ok(HordeUserInfoDto {
                anonymous: true,
                user: None,
                shared_key: None,
            });
        };

        let (user, shared_key) =
            tokio::try_join!(horde.find_user(&api_key), horde.shared_key(&api_key))?;
        Ok(HordeUserInfoDto {
            anonymous: false,
            user: Some(user),
            shared_key,
        })
    }

    /// The task is queued before the stream starts, so cancelling the stream can always delete it.
    pub async fn start_text_stream(
        &self,
        stream_id: String,
        dto: HordeTextGenerationRequestDto,
    ) -> Result<(), ApplicationError> {
        let horde = self.horde()?.clone();
        let mut payload = dto.payload;
        let quiet = is_quiet(&payload);
        payload.remove("type");
        let api_key = self
            .api_key()
            .await?
            .unwrap_or_else(|| ANONYMOUS_API_KEY.to_string());

        let task = horde
            .submit_text_generation(&Value::Object(payload), &api_key)
            .await?;
        let task_id = text::task_id(&task)?;
        let on_interrupt = text::cancel_task(horde.clone(), task_id.clone());

        self.generation_runtime
            .start_stream(
                stream_id,
                quiet,
                Some(on_interrupt),
                move |sender, _cancel| async move {
                    text::follow_task(horde.as_ref(), &task_id, &sender).await
                },
            )
            .await
    }

    /// Every Horde request takes the repository here, where the iOS policy is checked.
    fn horde(&self) -> Result<&Arc<dyn HordeRepository>, ApplicationError> {
        if self.ios_policy.scope == IosPolicyScope::Ios && !self.ios_policy.capabilities.ai.horde {
            return Err(ApplicationError::PermissionDenied(
                "iOS policy disabled capability: ai.horde".to_string(),
            ));
        }
        Ok(&self.repository)
    }

    async fn api_key(&self) -> Result<Option<String>, ApplicationError> {
        read_selected_secret(self.secret_repository.as_ref(), SecretKeys::HORDE, None).await
    }
}

/// The lock is held across the fetch, so concurrent readers wait for one fetch.
#[derive(Default)]
struct CachedList(Mutex<Option<(Instant, Value)>>);

impl CachedList {
    async fn get_or_fetch<F, Fut>(&self, force: bool, fetch: F) -> Result<Value, ApplicationError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Value, ApplicationError>>,
    {
        let mut cached = self.0.lock().await;
        if !force
            && let Some((fetched_at, list)) = cached.as_ref()
            && fetched_at.elapsed() < LIST_CACHE_TTL
        {
            return Ok(list.clone());
        }

        let list = fetch().await?;
        *cached = Some((Instant::now(), list.clone()));
        Ok(list)
    }
}

fn with_reference(models: Vec<Value>, reference: &Map<String, Value>) -> Vec<Value> {
    models
        .into_iter()
        .map(|mut model| {
            let entry = model
                .get("name")
                .and_then(Value::as_str)
                .and_then(|name| reference.get(name))
                .and_then(Value::as_object)
                .cloned();
            if let Value::Object(fields) = &mut model {
                let listed = entry.is_some();
                fields.extend(entry.into_iter().flatten());
                fields.insert("is_whitelisted".to_string(), Value::Bool(listed));
            }
            model
        })
        .collect()
}

#[cfg(test)]
mod tests;
