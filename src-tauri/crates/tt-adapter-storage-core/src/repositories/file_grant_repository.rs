use std::path::PathBuf;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_ports::repositories::grant_repository::GrantRepository;

use crate::file_system::{read_json_file, write_json_file};

pub struct FileGrantRepository {
    path: PathBuf,
}

impl FileGrantRepository {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl GrantRepository for FileGrantRepository {
    async fn load(&self) -> Result<Vec<String>, DomainError> {
        match read_json_file(&self.path).await {
            Ok(grants) => Ok(grants),
            Err(DomainError::NotFound(_)) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    async fn replace(&self, grants: &[String]) -> Result<(), DomainError> {
        write_json_file(&self.path, grants).await
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::file_system::unique_temp_path;

    #[tokio::test]
    async fn missing_file_is_empty_and_grants_round_trip() {
        let root = unique_temp_path(&std::env::temp_dir().join("grants"));
        let path = root.join("grants.json");
        let repository = FileGrantRepository::new(path);

        assert!(repository.load().await.unwrap().is_empty());

        let grants = vec!["https://example.com".to_string()];
        repository.replace(&grants).await.unwrap();

        assert_eq!(repository.load().await.unwrap(), grants);
        fs::remove_dir_all(root).unwrap();
    }
}
