use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use tt_domain::models::extension::{
    Extension, ExtensionBranch, ExtensionInstallResult, ExtensionUpdateResult, ExtensionVersion,
};
use tt_ports::repositories::extension_repository::ExtensionRepository;

use crate::errors::ApplicationError;

/// Application boundary for extension operations.
pub struct ExtensionService {
    extension_repository: Arc<dyn ExtensionRepository>,
    local_mutation_gate: Arc<Semaphore>,
}

impl ExtensionService {
    pub fn new(
        extension_repository: Arc<dyn ExtensionRepository>,
        local_mutation_gate: Arc<Semaphore>,
    ) -> Self {
        Self {
            extension_repository,
            local_mutation_gate,
        }
    }

    pub async fn get_extensions(&self) -> Result<Vec<Extension>, ApplicationError> {
        tracing::debug!("Getting all extensions");
        self.extension_repository
            .discover_extensions()
            .await
            .map_err(Into::into)
    }

    pub async fn install_extension(
        &self,
        url: &str,
        global: bool,
        branch: Option<String>,
    ) -> Result<ExtensionInstallResult, ApplicationError> {
        tracing::debug!("Installing extension");
        let _permit = self.try_mutation_permit()?;
        self.extension_repository
            .install_extension(url, global, branch)
            .await
            .map_err(Into::into)
    }

    pub async fn update_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionUpdateResult, ApplicationError> {
        tracing::debug!("Updating extension: {}", extension_name);
        let _permit = self.try_mutation_permit()?;
        self.extension_repository
            .update_extension(extension_name, global)
            .await
            .map_err(Into::into)
    }

    pub async fn delete_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<(), ApplicationError> {
        tracing::debug!("Deleting extension: {}", extension_name);
        let _permit = self.try_mutation_permit()?;
        self.extension_repository
            .delete_extension(extension_name, global)
            .await
            .map_err(Into::into)
    }

    pub async fn get_extension_version(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionVersion, ApplicationError> {
        tracing::debug!("Getting extension version: {}", extension_name);
        self.extension_repository
            .get_extension_version(extension_name, global)
            .await
            .map_err(Into::into)
    }

    pub async fn get_extension_branches(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<Vec<ExtensionBranch>, ApplicationError> {
        tracing::debug!("Getting extension branches: {}", extension_name);
        self.extension_repository
            .get_extension_branches(extension_name, global)
            .await
            .map_err(Into::into)
    }

    pub async fn switch_extension_branch(
        &self,
        extension_name: &str,
        branch: &str,
        global: bool,
    ) -> Result<(), ApplicationError> {
        tracing::debug!(
            "Switching extension {} to branch {}",
            extension_name,
            branch
        );
        let _permit = self.try_mutation_permit()?;
        self.extension_repository
            .switch_extension_branch(extension_name, branch, global)
            .await
            .map_err(Into::into)
    }

    pub async fn move_extension(
        &self,
        extension_name: &str,
        source: &str,
        destination: &str,
    ) -> Result<(), ApplicationError> {
        tracing::debug!(
            "Moving extension: {} from {} to {}",
            extension_name,
            source,
            destination
        );
        let _permit = self.try_mutation_permit()?;
        self.extension_repository
            .move_extension(extension_name, source, destination)
            .await
            .map_err(Into::into)
    }

    fn try_mutation_permit(&self) -> Result<OwnedSemaphorePermit, ApplicationError> {
        self.local_mutation_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                ApplicationError::Conflict("Another local mutation is already running".to_string())
            })
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use tt_domain::errors::DomainError;

    use super::*;

    struct ReadOnlyRepository;

    #[async_trait]
    impl ExtensionRepository for ReadOnlyRepository {
        async fn discover_extensions(&self) -> Result<Vec<Extension>, DomainError> {
            Ok(Vec::new())
        }

        async fn install_extension(
            &self,
            _url: &str,
            _global: bool,
            _branch: Option<String>,
        ) -> Result<ExtensionInstallResult, DomainError> {
            unreachable!()
        }

        async fn update_extension(
            &self,
            _extension_name: &str,
            _global: bool,
        ) -> Result<ExtensionUpdateResult, DomainError> {
            unreachable!()
        }

        async fn delete_extension(
            &self,
            _extension_name: &str,
            _global: bool,
        ) -> Result<(), DomainError> {
            unreachable!()
        }

        async fn get_extension_version(
            &self,
            _extension_name: &str,
            _global: bool,
        ) -> Result<ExtensionVersion, DomainError> {
            Ok(ExtensionVersion {
                current_branch_name: String::new(),
                current_commit_hash: String::new(),
                is_up_to_date: true,
                remote_url: String::new(),
            })
        }

        async fn get_extension_branches(
            &self,
            _extension_name: &str,
            _global: bool,
        ) -> Result<Vec<ExtensionBranch>, DomainError> {
            Ok(Vec::new())
        }

        async fn switch_extension_branch(
            &self,
            _extension_name: &str,
            _branch: &str,
            _global: bool,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        async fn move_extension(
            &self,
            _extension_name: &str,
            _source: &str,
            _destination: &str,
        ) -> Result<(), DomainError> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn shared_gate_is_fail_fast_for_mutations_but_not_reads() {
        let gate = Arc::new(Semaphore::new(1));
        let held = gate.clone().try_acquire_owned().unwrap();
        let service = ExtensionService::new(Arc::new(ReadOnlyRepository), gate);

        assert!(matches!(
            service.try_mutation_permit(),
            Err(ApplicationError::Conflict(_))
        ));
        assert!(service.get_extensions().await.is_ok());
        assert!(
            service
                .get_extension_version("extension", false)
                .await
                .is_ok()
        );
        assert!(
            service
                .get_extension_branches("extension", false)
                .await
                .is_ok()
        );
        assert!(matches!(
            service
                .switch_extension_branch("extension", "main", false)
                .await,
            Err(ApplicationError::Conflict(_))
        ));

        drop(held);
        assert!(service.try_mutation_permit().is_ok());
    }
}
