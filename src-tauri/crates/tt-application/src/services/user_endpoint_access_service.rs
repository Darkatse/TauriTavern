use std::sync::Arc;

use tokio::sync::Mutex;
use tt_ports::repositories::user_endpoint_grant_repository::UserEndpointGrantRepository;
use tt_ports::user_endpoint_access::UserEndpointGrantRuntime;

pub struct UserEndpointAccessService {
    repository: Arc<dyn UserEndpointGrantRepository>,
    runtime: Arc<dyn UserEndpointGrantRuntime>,
    grants: Mutex<Vec<String>>,
}

impl UserEndpointAccessService {
    pub async fn initialize(
        repository: Arc<dyn UserEndpointGrantRepository>,
        runtime: Arc<dyn UserEndpointGrantRuntime>,
    ) -> Self {
        let grants = match repository.load().await {
            Ok(endpoints) => endpoints,
            Err(error) => {
                tracing::warn!(%error, "User endpoint grants could not be loaded; starting with none");
                Vec::new()
            }
        };
        runtime.replace_user_endpoint_grants(&grants);

        Self {
            repository,
            runtime,
            grants: Mutex::new(grants),
        }
    }

    pub async fn is_granted(&self, endpoint: &str) -> bool {
        let grants = self.grants.lock().await;
        grants.iter().any(|grant| grant == endpoint)
    }

    pub async fn grant(&self, endpoint: String) {
        let mut grants = self.grants.lock().await;
        if grants.contains(&endpoint) {
            return;
        }
        grants.push(endpoint.clone());

        self.runtime.replace_user_endpoint_grants(&grants);

        if let Err(error) = self.repository.replace(&grants).await {
            tracing::warn!(
                %error,
                %endpoint,
                "User endpoint grant is active for this session but could not be persisted"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use tt_domain::errors::DomainError;

    use super::*;

    #[derive(Default)]
    struct MemoryRepository(StdMutex<Vec<String>>);

    #[async_trait]
    impl UserEndpointGrantRepository for MemoryRepository {
        async fn load(&self) -> Result<Vec<String>, DomainError> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn replace(&self, endpoints: &[String]) -> Result<(), DomainError> {
            *self.0.lock().unwrap() = endpoints.to_vec();
            Ok(())
        }
    }

    struct FailingRepository;

    #[async_trait]
    impl UserEndpointGrantRepository for FailingRepository {
        async fn load(&self) -> Result<Vec<String>, DomainError> {
            Ok(Vec::new())
        }

        async fn replace(&self, _endpoints: &[String]) -> Result<(), DomainError> {
            Err(DomainError::InternalError("write failed".to_string()))
        }
    }

    #[derive(Default)]
    struct RecordingRuntime(StdMutex<Vec<String>>);

    impl UserEndpointGrantRuntime for RecordingRuntime {
        fn replace_user_endpoint_grants(&self, endpoints: &[String]) {
            *self.0.lock().unwrap() = endpoints.to_vec();
        }
    }

    #[tokio::test]
    async fn grant_is_persisted_once_and_immediately_applied() {
        let endpoint = "https://api.example.com/v1".to_string();
        let repository = Arc::new(MemoryRepository::default());
        let runtime = Arc::new(RecordingRuntime::default());
        let service =
            UserEndpointAccessService::initialize(repository.clone(), runtime.clone()).await;

        assert!(!service.is_granted(&endpoint).await);
        service.grant(endpoint.clone()).await;

        assert!(service.is_granted(&endpoint).await);
        assert_eq!(*repository.0.lock().unwrap(), vec![endpoint.clone()]);
        assert_eq!(*runtime.0.lock().unwrap(), vec![endpoint]);
    }

    #[tokio::test]
    async fn failed_persistence_keeps_session_access() {
        let endpoint = "https://api.example.com/v1".to_string();
        let runtime = Arc::new(RecordingRuntime::default());
        let service =
            UserEndpointAccessService::initialize(Arc::new(FailingRepository), runtime.clone())
                .await;

        service.grant(endpoint.clone()).await;

        assert!(service.is_granted(&endpoint).await);
        assert_eq!(*runtime.0.lock().unwrap(), vec![endpoint]);
    }
}
