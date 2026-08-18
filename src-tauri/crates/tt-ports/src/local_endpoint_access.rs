use async_trait::async_trait;
use tt_domain::errors::DomainError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEndpointCandidate {
    pub endpoint: String,
    pub addresses: Vec<String>,
}

#[async_trait]
pub trait LocalEndpointAccessRuntime: Send + Sync {
    async fn inspect_user_endpoint(
        &self,
        endpoint: &str,
    ) -> Result<Option<LocalEndpointCandidate>, DomainError>;

    fn replace_local_endpoint_grants(&self, endpoints: &[String]);
}
