use std::sync::Arc;

use async_trait::async_trait;
use tt_ports::repositories::grant_repository::GrantRepository;
use tt_ports::user_endpoint_access::UserEndpointGrantRuntime;

use crate::services::user_grants::{GrantList, UserGrants};

pub struct UserEndpointAccessService {
    grants: GrantList,
    runtime: Arc<dyn UserEndpointGrantRuntime>,
}

impl UserEndpointAccessService {
    pub async fn initialize(
        repository: Arc<dyn GrantRepository>,
        runtime: Arc<dyn UserEndpointGrantRuntime>,
    ) -> Self {
        let grants = GrantList::load("user endpoint", repository).await;
        runtime.allow_user_endpoints(&grants.all().await);
        Self { grants, runtime }
    }
}

#[async_trait]
impl UserGrants for UserEndpointAccessService {
    async fn is_allowed(&self, endpoint: &str) -> bool {
        self.grants.contains(endpoint).await
    }

    /// The HTTP pool learns of the grant first, so whoever sees it granted can already use it.
    async fn grant(&self, endpoint: &str) {
        self.runtime.allow_user_endpoints(&[endpoint.to_string()]);
        self.grants.grant(endpoint).await;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use super::*;
    use crate::services::user_grants::tests::MemoryGrantRepository;

    #[derive(Default)]
    struct RecordingRuntime(Mutex<HashSet<String>>);

    impl UserEndpointGrantRuntime for RecordingRuntime {
        fn allow_user_endpoints(&self, endpoints: &[String]) {
            self.0.lock().unwrap().extend(endpoints.iter().cloned());
        }
    }

    #[tokio::test]
    async fn saved_and_new_grants_reach_the_runtime_as_origins() {
        let repository = Arc::new(MemoryGrantRepository(Mutex::new(vec![
            "https://saved.example/v1".to_string(),
            "https://saved.example/".to_string(),
        ])));
        let runtime = Arc::new(RecordingRuntime::default());
        let service =
            UserEndpointAccessService::initialize(repository.clone(), runtime.clone()).await;

        service.grant("https://new.example").await;

        let origins = ["https://saved.example", "https://new.example"].map(String::from);
        assert_eq!(*runtime.0.lock().unwrap(), HashSet::from(origins.clone()));
        assert_eq!(*repository.0.lock().unwrap(), origins);
    }
}
