use std::path::Path;

use async_trait::async_trait;

use crate::application::services::runtime_paths_service::{
    RuntimePathConfigInfo, RuntimePathConfigStore,
};
use crate::domain::errors::DomainError;
use crate::infrastructure::paths::{load_runtime_config, request_runtime_data_root_change};

pub(crate) struct FilesystemRuntimePathConfigStore;

#[async_trait]
impl RuntimePathConfigStore for FilesystemRuntimePathConfigStore {
    fn load_config(&self, app_root: &Path) -> Result<Option<RuntimePathConfigInfo>, DomainError> {
        load_runtime_config(app_root)
            .map(|config| {
                config.map(|config| RuntimePathConfigInfo {
                    data_root: config.data_root,
                    migration_pending: config.migration.is_some(),
                    migration_error: config.migration_error,
                })
            })
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to load tauritavern-runtime.json: {error}"
                ))
            })
    }

    async fn request_data_root_change(
        &self,
        app_root: &Path,
        current_data_root: &Path,
        raw_target: &str,
    ) -> Result<(), DomainError> {
        request_runtime_data_root_change(app_root, current_data_root, raw_target).await
    }
}
