use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use yup_oauth2::ServiceAccountAuthenticator;
use yup_oauth2::ServiceAccountKey;
use yup_oauth2::authenticator::Authenticator;
use yup_oauth2::client::DefaultHyperClientBuilder;
#[cfg(target_os = "android")]
use yup_oauth2::client::CustomHyperClientBuilder;
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
            let service_account_key = serde_json::from_str::<ServiceAccountKey>(
                service_account_json,
            )
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

            let authenticator =
                build_service_account_authenticator(service_account_key)
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
        ApplicationError::InternalError(
            "Vertex AI access token response is missing token".to_string(),
        )
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

async fn build_service_account_authenticator(
    service_account_key: ServiceAccountKey,
) -> Result<DefaultAuthenticator, std::io::Error> {
    #[cfg(target_os = "android")]
    {
        ServiceAccountAuthenticator::with_client(service_account_key, build_android_hyper_client())
            .build()
            .await
    }

    #[cfg(not(target_os = "android"))]
    {
        ServiceAccountAuthenticator::builder(service_account_key)
            .build()
            .await
    }
}

#[cfg(target_os = "android")]
fn build_android_hyper_client() -> CustomHyperClientBuilder<
    yup_oauth2::hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
> {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };

    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = yup_oauth2::hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let client =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .pool_max_idle_per_host(0)
            .build::<_, String>(connector);

    CustomHyperClientBuilder::from(client)
}
