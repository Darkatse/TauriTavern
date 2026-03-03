use std::sync::Arc;

use crate::domain::errors::DomainError;
use crate::domain::models::update::UpdateCheckResult;
use crate::domain::repositories::update_repository::UpdateRepository;

pub struct UpdateService {
    repository: Arc<dyn UpdateRepository>,
}

impl UpdateService {
    pub fn new(repository: Arc<dyn UpdateRepository>) -> Self {
        Self { repository }
    }

    pub async fn check_for_update(&self) -> Result<UpdateCheckResult, DomainError> {
        let current_version = env!("CARGO_PKG_VERSION");
        let latest_release = self.repository.get_latest_release().await?;

        let has_update = is_newer_version(current_version, &latest_release.version);

        Ok(UpdateCheckResult {
            has_update,
            current_version: current_version.to_string(),
            latest_release: if has_update {
                Some(latest_release)
            } else {
                None
            },
        })
    }
}

fn is_newer_version(local: &str, remote: &str) -> bool {
    let parse = |value: &str| -> Vec<u64> {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };

    let local_parts = parse(local);
    let remote_parts = parse(remote);

    for index in 0..local_parts.len().max(remote_parts.len()) {
        let left = local_parts.get(index).copied().unwrap_or(0);
        let right = remote_parts.get(index).copied().unwrap_or(0);

        if right > left {
            return true;
        }
        if right < left {
            return false;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::is_newer_version;

    #[test]
    fn newer_patch_version() {
        assert!(is_newer_version("1.3.0", "1.3.1"));
    }

    #[test]
    fn newer_minor_version() {
        assert!(is_newer_version("1.3.0", "1.4.0"));
    }

    #[test]
    fn newer_major_version() {
        assert!(is_newer_version("1.3.0", "2.0.0"));
    }

    #[test]
    fn same_version() {
        assert!(!is_newer_version("1.3.0", "1.3.0"));
    }

    #[test]
    fn older_version() {
        assert!(!is_newer_version("1.3.0", "1.2.9"));
    }

    #[test]
    fn different_segment_lengths() {
        assert!(is_newer_version("1.3", "1.3.1"));
        assert!(!is_newer_version("1.3.1", "1.3"));
    }
}
