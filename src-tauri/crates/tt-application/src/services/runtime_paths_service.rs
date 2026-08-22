use std::sync::Arc;

use tt_domain::errors::DomainError;
pub use tt_ports::runtime_paths::{
    RuntimeModeInfo, RuntimePathConfigInfo, RuntimePathConfigStore, RuntimePathsInfo,
    RuntimePathsSnapshot,
};

#[derive(Clone)]
pub struct RuntimePathsService {
    runtime_paths: RuntimePathsSnapshot,
    store: Arc<dyn RuntimePathConfigStore>,
}

impl RuntimePathsService {
    pub fn new<S>(runtime_paths: RuntimePathsSnapshot, store: Arc<S>) -> Self
    where
        S: RuntimePathConfigStore + 'static,
    {
        let store: Arc<dyn RuntimePathConfigStore> = store;
        Self {
            runtime_paths,
            store,
        }
    }

    pub fn get_runtime_paths(&self) -> Result<RuntimePathsInfo, DomainError> {
        let config = self.store.load_config(&self.runtime_paths.app_root)?;

        Ok(RuntimePathsInfo {
            mode: self.runtime_paths.mode,
            data_root: self.runtime_paths.data_root.clone(),
            configured_data_root: config.as_ref().map(|config| config.data_root.clone()),
            migration_pending: config
                .as_ref()
                .is_some_and(|config| config.migration_pending),
            migration_error: config.and_then(|config| config.migration_error),
        })
    }

    pub async fn request_data_root_change(&self, raw: &str) -> Result<(), DomainError> {
        self.store
            .request_data_root_change(
                &self.runtime_paths.app_root,
                &self.runtime_paths.data_root,
                raw,
            )
            .await
    }
}
