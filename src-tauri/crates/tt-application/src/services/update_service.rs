use std::sync::Arc;

use tt_domain::errors::DomainError;
use tt_domain::models::update::UpdateCheckResult;
use tt_ports::repositories::update_repository::UpdateRepository;

pub struct UpdateService {
    repository: Arc<dyn UpdateRepository>,
    current_version: String,
}

impl UpdateService {
    pub fn new(repository: Arc<dyn UpdateRepository>, current_version: impl Into<String>) -> Self {
        Self {
            repository,
            current_version: current_version.into(),
        }
    }

    pub async fn check_for_update(&self) -> Result<UpdateCheckResult, DomainError> {
        let latest_release = self.repository.get_latest_release().await?;
        let has_update = is_newer_version(&self.current_version, &latest_release.version)?;

        Ok(UpdateCheckResult {
            has_update,
            current_version: self.current_version.clone(),
            latest_release: if has_update {
                Some(latest_release)
            } else {
                None
            },
        })
    }
}

fn is_newer_version(local: &str, remote: &str) -> Result<bool, DomainError> {
    let local_parts = parse_version(local)?;
    let remote_parts = parse_version(remote)?;

    for index in 0..local_parts.len().max(remote_parts.len()) {
        let left = local_parts.get(index).copied().unwrap_or(0);
        let right = remote_parts.get(index).copied().unwrap_or(0);

        if right > left {
            return Ok(true);
        }
        if right < left {
            return Ok(false);
        }
    }

    Ok(false)
}

fn parse_version(value: &str) -> Result<Vec<u64>, DomainError> {
    let value = value.trim();

    if value.is_empty() {
        return Err(invalid_version(value));
    }

    value
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.chars().all(|character| character.is_ascii_digit()) {
                return Err(invalid_version(value));
            }

            part.parse::<u64>().map_err(|_| invalid_version(value))
        })
        .collect()
}

fn invalid_version(value: &str) -> DomainError {
    DomainError::InvalidData(format!("Invalid app version: {value}"))
}

#[cfg(test)]
mod tests {
    use super::{UpdateService, is_newer_version};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tt_domain::errors::DomainError;
    use tt_domain::models::update::ReleaseInfo;
    use tt_ports::repositories::update_repository::UpdateRepository;

    struct FakeUpdateRepository {
        latest_release: ReleaseInfo,
    }

    #[async_trait]
    impl UpdateRepository for FakeUpdateRepository {
        async fn get_latest_release(&self) -> Result<ReleaseInfo, DomainError> {
            Ok(self.latest_release.clone())
        }
    }

    #[test]
    fn newer_patch_version() {
        assert!(is_newer_version("1.3.0", "1.3.1").unwrap());
    }

    #[test]
    fn newer_minor_version() {
        assert!(is_newer_version("1.3.0", "1.4.0").unwrap());
    }

    #[test]
    fn newer_major_version() {
        assert!(is_newer_version("1.3.0", "2.0.0").unwrap());
    }

    #[test]
    fn same_version() {
        assert!(!is_newer_version("1.3.0", "1.3.0").unwrap());
    }

    #[test]
    fn older_version() {
        assert!(!is_newer_version("1.3.0", "1.2.9").unwrap());
    }

    #[test]
    fn different_segment_lengths() {
        assert!(is_newer_version("1.3", "1.3.1").unwrap());
        assert!(!is_newer_version("1.3.1", "1.3").unwrap());
    }

    #[test]
    fn invalid_version_fails_fast() {
        assert!(is_newer_version("1.x.0", "1.2.0").is_err());
        assert!(is_newer_version("1.2.0", "latest").is_err());
    }

    #[tokio::test]
    async fn check_for_update_uses_injected_product_version() {
        let repository = Arc::new(FakeUpdateRepository {
            latest_release: release("2.1.1"),
        });
        let service = UpdateService::new(repository, "2.1.1");

        let result = service.check_for_update().await.unwrap();

        assert_eq!(result.current_version, "2.1.1");
        assert!(!result.has_update);
        assert!(result.latest_release.is_none());
    }

    fn release(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: format!("v{version}"),
            version: version.to_string(),
            name: format!("v{version}"),
            body: String::new(),
            html_url: String::new(),
            prerelease: false,
            published_at: String::new(),
        }
    }
}
