use std::path::PathBuf;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_ports::repositories::user_endpoint_grant_repository::UserEndpointGrantRepository;

use crate::file_system::{read_json_file, write_json_file};

pub struct FileUserEndpointGrantRepository {
    path: PathBuf,
}

impl FileUserEndpointGrantRepository {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl UserEndpointGrantRepository for FileUserEndpointGrantRepository {
    async fn load(&self) -> Result<Vec<String>, DomainError> {
        match read_json_file(&self.path).await {
            Ok(endpoints) => Ok(endpoints),
            Err(DomainError::NotFound(_)) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    async fn replace(&self, endpoints: &[String]) -> Result<(), DomainError> {
        write_json_file(&self.path, endpoints).await
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::file_system::unique_temp_path;

    #[tokio::test]
    async fn missing_file_is_empty_and_grants_round_trip() {
        let root = unique_temp_path(&std::env::temp_dir().join("user-endpoint-grants"));
        let path = root.join("user-endpoint-grants.json");
        let repository = FileUserEndpointGrantRepository::new(path);

        assert!(repository.load().await.unwrap().is_empty());

        let endpoints = vec!["https://api.example.com/v1".to_string()];
        repository.replace(&endpoints).await.unwrap();

        assert_eq!(repository.load().await.unwrap(), endpoints);
        fs::remove_dir_all(root).unwrap();
    }
}
