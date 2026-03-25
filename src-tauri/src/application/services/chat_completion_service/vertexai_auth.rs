use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use yup_oauth2::ServiceAccountAuthenticator;
use yup_oauth2::ServiceAccountKey;
use yup_oauth2::authenticator::Authenticator;
use yup_oauth2::client::DefaultHyperClientBuilder;
use yup_oauth2::client::HyperClientBuilder;

use crate::application::errors::ApplicationError;

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

type DefaultAuthenticator =
    Authenticator<<DefaultHyperClientBuilder as HyperClientBuilder>::Connector>;

#[derive(Clone)]
struct CachedServiceAccount {
    project_id: String,
    authenticator: Arc<DefaultAuthenticator>,
}

static SERVICE_ACCOUNT_CACHE: OnceLock<RwLock<HashMap<String, CachedServiceAccount>>> =
    OnceLock::new();

pub(super) async fn get_service_account_access_token(
    service_account_json: &str,
) -> Result<(String, String), ApplicationError> {
    let cache_key = sha256_hex(service_account_json);

    let cached = {
        let cache = service_account_cache().read().await;
        cache.get(&cache_key).cloned()
    };

    let cached = match cached {
        Some(cached) => cached,
        None => {
            let service_account_key = serde_json::from_str::<ServiceAccountKey>(service_account_json)
                .map_err(|error| {
                    ApplicationError::ValidationError(format!(
                        "Vertex AI service account JSON parse failed: {error}"
                    ))
                })?;

            let project_id = service_account_key
                .project_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "Vertex AI service account JSON is missing project_id".to_string(),
                    )
                })?
                .to_string();

            let authenticator = ServiceAccountAuthenticator::builder(service_account_key)
                .build()
                .await
                .map_err(|error| {
                    ApplicationError::InternalError(format!(
                        "Vertex AI service account authenticator build failed: {error}"
                    ))
                })?;

            let cached = CachedServiceAccount {
                project_id,
                authenticator: Arc::new(authenticator),
            };

            let mut cache = service_account_cache().write().await;
            cache.insert(cache_key, cached.clone());
            cached
        }
    };

    let token = cached
        .authenticator
        .token(&[CLOUD_PLATFORM_SCOPE])
        .await
        .map_err(|error| {
            ApplicationError::InternalError(format!(
                "Vertex AI service account access token request failed: {error}"
            ))
        })?;

    let access_token = token.token().ok_or_else(|| {
        ApplicationError::InternalError("Vertex AI access token response is missing token".to_string())
    })?;

    Ok((cached.project_id, access_token.to_string()))
}

fn service_account_cache() -> &'static RwLock<HashMap<String, CachedServiceAccount>> {
    SERVICE_ACCOUNT_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn sha256_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    format!("{digest:x}")
}

