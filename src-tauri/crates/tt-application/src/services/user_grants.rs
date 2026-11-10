use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tt_ports::repositories::grant_repository::GrantRepository;

#[async_trait]
pub trait UserGrants: Send + Sync {
    async fn is_allowed(&self, subject: &str) -> bool;

    async fn grant(&self, subject: &str);
}

/// A grant holds for the session even when it cannot be saved.
pub(crate) struct GrantList {
    kind: &'static str,
    repository: Arc<dyn GrantRepository>,
    grants: Mutex<Vec<String>>,
}

impl GrantList {
    pub(crate) async fn load(kind: &'static str, repository: Arc<dyn GrantRepository>) -> Self {
        let grants = repository.load().await.unwrap_or_else(|error| {
            tracing::warn!(%error, kind, "Grants could not be loaded; starting with none");
            Vec::new()
        });
        Self {
            kind,
            repository,
            grants: Mutex::new(grants),
        }
    }

    pub(crate) async fn all(&self) -> Vec<String> {
        self.grants.lock().await.clone()
    }

    pub(crate) async fn contains(&self, grant: &str) -> bool {
        self.grants
            .lock()
            .await
            .iter()
            .any(|existing| existing == grant)
    }

    pub(crate) async fn grant(&self, grant: &str) {
        let mut grants = self.grants.lock().await;
        if grants.iter().any(|existing| existing == grant) {
            return;
        }
        grants.push(grant.to_string());

        if let Err(error) = self.repository.replace(&grants).await {
            tracing::warn!(
                %error,
                kind = self.kind,
                grant,
                "Grant is active for this session but could not be saved"
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use tt_domain::errors::DomainError;

    use super::*;

    #[derive(Default)]
    pub(crate) struct MemoryGrantRepository(pub(crate) StdMutex<Vec<String>>);

    #[async_trait]
    impl GrantRepository for MemoryGrantRepository {
        async fn load(&self) -> Result<Vec<String>, DomainError> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn replace(&self, grants: &[String]) -> Result<(), DomainError> {
            *self.0.lock().unwrap() = grants.to_vec();
            Ok(())
        }
    }

    struct FailingRepository;

    #[async_trait]
    impl GrantRepository for FailingRepository {
        async fn load(&self) -> Result<Vec<String>, DomainError> {
            Err(DomainError::InternalError("read failed".to_string()))
        }

        async fn replace(&self, _grants: &[String]) -> Result<(), DomainError> {
            Err(DomainError::InternalError("write failed".to_string()))
        }
    }

    #[tokio::test]
    async fn grant_is_saved_once() {
        let repository = Arc::new(MemoryGrantRepository::default());
        let grants = GrantList::load("test", repository.clone()).await;

        grants.grant("https://example.com").await;
        grants.grant("https://example.com").await;

        assert!(grants.contains("https://example.com").await);
        assert_eq!(
            *repository.0.lock().unwrap(),
            vec!["https://example.com".to_string()]
        );
    }

    #[tokio::test]
    async fn unsaved_grant_holds_for_the_session() {
        let grants = GrantList::load("test", Arc::new(FailingRepository)).await;

        grants.grant("https://example.com").await;

        assert!(grants.contains("https://example.com").await);
    }
}
