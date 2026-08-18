use std::collections::BTreeSet;
use std::sync::Arc;

use tokio::sync::Mutex;
use tt_ports::local_endpoint_access::{LocalEndpointAccessRuntime, LocalEndpointCandidate};
use tt_ports::repositories::local_endpoint_grant_repository::LocalEndpointGrantRepository;

use crate::errors::ApplicationError;

pub struct LocalEndpointAccessService {
    repository: Arc<dyn LocalEndpointGrantRepository>,
    runtime: Arc<dyn LocalEndpointAccessRuntime>,
    grants: Mutex<BTreeSet<String>>,
}

impl LocalEndpointAccessService {
    pub async fn initialize(
        repository: Arc<dyn LocalEndpointGrantRepository>,
        runtime: Arc<dyn LocalEndpointAccessRuntime>,
    ) -> Self {
        let grants = match repository.load().await {
            Ok(endpoints) => endpoints.into_iter().collect(),
            Err(error) => {
                tracing::warn!(%error, "Local endpoint grants could not be loaded; starting with none");
                BTreeSet::new()
            }
        };
        runtime.replace_local_endpoint_grants(&grants.iter().cloned().collect::<Vec<_>>());

        Self {
            repository,
            runtime,
            grants: Mutex::new(grants),
        }
    }

    pub async fn authorization_candidate(
        &self,
        endpoint: &str,
    ) -> Result<Option<LocalEndpointCandidate>, ApplicationError> {
        if self.grants.lock().await.contains(endpoint) {
            return Ok(None);
        }

        self.runtime
            .inspect_user_endpoint(endpoint)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn grant(&self, endpoint: String) {
        let mut grants = self.grants.lock().await;
        if grants.contains(&endpoint) {
            return;
        }

        let mut next = grants.clone();
        next.insert(endpoint.clone());
        let endpoints = next.iter().cloned().collect::<Vec<_>>();
        self.runtime.replace_local_endpoint_grants(&endpoints);
        *grants = next;

        if let Err(error) = self.repository.replace(&endpoints).await {
            tracing::warn!(
                %error,
                %endpoint,
                "Local endpoint grant is active for this session but could not be persisted"
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
    impl LocalEndpointGrantRepository for MemoryRepository {
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
    impl LocalEndpointGrantRepository for FailingRepository {
        async fn load(&self) -> Result<Vec<String>, DomainError> {
            Ok(Vec::new())
        }

        async fn replace(&self, _endpoints: &[String]) -> Result<(), DomainError> {
            Err(DomainError::InternalError("write failed".to_string()))
        }
    }

    struct FixedRuntime {
        candidate: LocalEndpointCandidate,
        grants: StdMutex<Vec<String>>,
    }

    #[async_trait]
    impl LocalEndpointAccessRuntime for FixedRuntime {
        async fn inspect_user_endpoint(
            &self,
            _endpoint: &str,
        ) -> Result<Option<LocalEndpointCandidate>, DomainError> {
            Ok(Some(self.candidate.clone()))
        }

        fn replace_local_endpoint_grants(&self, endpoints: &[String]) {
            *self.grants.lock().unwrap() = endpoints.to_vec();
        }
    }

    fn fixed_runtime(endpoint: &str) -> Arc<FixedRuntime> {
        Arc::new(FixedRuntime {
            candidate: LocalEndpointCandidate {
                endpoint: endpoint.to_string(),
                addresses: vec!["192.168.1.2".to_string()],
            },
            grants: StdMutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn grant_is_persisted_once_and_immediately_applied() {
        let endpoint = "http://lan.example:11434/v1".to_string();
        let repository = Arc::new(MemoryRepository::default());
        let runtime = fixed_runtime(&endpoint);
        let service =
            LocalEndpointAccessService::initialize(repository.clone(), runtime.clone()).await;

        assert!(
            service
                .authorization_candidate(&endpoint)
                .await
                .unwrap()
                .is_some()
        );
        service.grant(endpoint.clone()).await;
        assert!(
            service
                .authorization_candidate(&endpoint)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(*repository.0.lock().unwrap(), vec![endpoint.clone()]);
        assert_eq!(*runtime.grants.lock().unwrap(), vec![endpoint]);
    }

    #[tokio::test]
    async fn failed_persistence_keeps_session_access() {
        let endpoint = "http://lan.example:11434/v1".to_string();
        let runtime = fixed_runtime(&endpoint);
        let service =
            LocalEndpointAccessService::initialize(Arc::new(FailingRepository), runtime.clone())
                .await;

        service.grant(endpoint.clone()).await;
        assert_eq!(*runtime.grants.lock().unwrap(), vec![endpoint.clone()]);
        assert!(
            service
                .authorization_candidate(&endpoint)
                .await
                .unwrap()
                .is_none()
        );
    }
}
