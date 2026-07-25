use std::sync::Arc;

use tt_domain::errors::DomainError;
use tt_domain::models::update::{UpdateChannel, UpdateCheckResult};
use tt_ports::repositories::update_repository::UpdateRepository;

pub struct UpdateService {
    repository: Arc<dyn UpdateRepository>,
    current_version: String,
    current_revision: Option<String>,
}

impl UpdateService {
    pub fn new(
        repository: Arc<dyn UpdateRepository>,
        current_version: impl Into<String>,
        current_revision: Option<impl Into<String>>,
    ) -> Self {
        Self {
            repository,
            current_version: current_version.into(),
            current_revision: current_revision.map(Into::into),
        }
    }

    pub async fn check_for_update(
        &self,
        channel: UpdateChannel,
    ) -> Result<UpdateCheckResult, DomainError> {
        let latest_release = self.repository.get_release(channel).await?;
        let (has_update, release_token) = match channel {
            UpdateChannel::Stable => {
                let has_update = is_newer_version(
                    &self.current_version,
                    latest_release
                        .version
                        .as_deref()
                        .ok_or_else(|| missing_release_field(channel, "version"))?,
                )?;
                let token = has_update.then(|| format!("stable:{}", latest_release.tag_name));
                (has_update, token)
            }
            UpdateChannel::Canary => {
                let current_revision = parse_revision(
                    self.current_revision
                        .as_deref()
                        .ok_or_else(missing_current_revision)?,
                )?;
                let release_revision = parse_revision(
                    latest_release
                        .source_revision
                        .as_deref()
                        .ok_or_else(|| missing_release_field(channel, "source revision"))?,
                )?;
                let has_update = current_revision != release_revision;
                let token = has_update.then(|| format!("canary:{release_revision}"));
                (has_update, token)
            }
        };

        Ok(UpdateCheckResult {
            has_update,
            current_version: self.current_version.clone(),
            channel,
            release_token,
            latest_release: if has_update {
                Some(latest_release)
            } else {
                None
            },
        })
    }
}

fn parse_revision(value: &str) -> Result<&str, DomainError> {
    let value = value.trim();
    if !(12..=40).contains(&value.len())
        || !value.chars().all(|character| character.is_ascii_hexdigit())
    {
        return Err(DomainError::InvalidData(format!(
            "Invalid Git revision: {value}"
        )));
    }
    Ok(&value[..12])
}

fn missing_current_revision() -> DomainError {
    DomainError::InvalidData("Current build Git revision is unavailable".to_string())
}

fn missing_release_field(channel: UpdateChannel, field: &str) -> DomainError {
    DomainError::InvalidData(format!("{channel:?} release is missing {field}"))
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
    use super::{UpdateService, is_newer_version, parse_revision};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tt_domain::errors::DomainError;
    use tt_domain::models::update::{ReleaseInfo, UpdateChannel};
    use tt_ports::repositories::update_repository::UpdateRepository;

    struct FakeUpdateRepository {
        latest_release: ReleaseInfo,
    }

    #[async_trait]
    impl UpdateRepository for FakeUpdateRepository {
        async fn get_release(&self, _channel: UpdateChannel) -> Result<ReleaseInfo, DomainError> {
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

    #[test]
    fn revision_identity_is_exactly_sha12() {
        assert_eq!(
            parse_revision("34b126db7e23490330ef4a8ea622d36c70bc831c").unwrap(),
            "34b126db7e23"
        );
        assert!(parse_revision("34b126d").is_err());
        assert!(parse_revision("not-a-sha12").is_err());
    }

    #[tokio::test]
    async fn check_for_update_uses_injected_product_version() {
        let repository = Arc::new(FakeUpdateRepository {
            latest_release: release("2.1.1"),
        });
        let service = UpdateService::new(repository, "2.1.1", Some("34b126db7e23"));

        let result = service
            .check_for_update(UpdateChannel::Stable)
            .await
            .unwrap();

        assert_eq!(result.current_version, "2.1.1");
        assert!(!result.has_update);
        assert!(result.latest_release.is_none());
    }

    #[tokio::test]
    async fn canary_uses_revision_instead_of_version() {
        let repository = Arc::new(FakeUpdateRepository {
            latest_release: canary_release("44b126db7e23490330ef4a8ea622d36c70bc831c"),
        });
        let service = UpdateService::new(repository, "2.1.1", Some("34b126db7e23"));

        let result = service
            .check_for_update(UpdateChannel::Canary)
            .await
            .unwrap();

        assert!(result.has_update);
        assert_eq!(result.release_token.as_deref(), Some("canary:44b126db7e23"));
    }

    fn release(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: format!("v{version}"),
            version: Some(version.to_string()),
            source_revision: None,
            name: format!("v{version}"),
            body: String::new(),
            html_url: String::new(),
            prerelease: false,
            published_at: String::new(),
        }
    }

    fn canary_release(revision: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: "Canary".to_string(),
            version: None,
            source_revision: Some(revision.to_string()),
            name: "Canary Release 2026.07.24".to_string(),
            body: String::new(),
            html_url: String::new(),
            prerelease: true,
            published_at: String::new(),
        }
    }
}
