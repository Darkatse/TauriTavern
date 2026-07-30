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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::*;
    use async_trait::async_trait;

    struct Store {
        config: Option<RuntimePathConfigInfo>,
        requests: Mutex<Vec<(PathBuf, PathBuf, String)>>,
    }

    impl Store {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                config: None,
                requests: Mutex::new(Vec::new()),
            })
        }

        fn with_config(mut self: Arc<Self>, config: RuntimePathConfigInfo) -> Arc<Self> {
            Arc::get_mut(&mut self)
                .expect("store must be uniquely owned")
                .config = Some(config);
            self
        }

        fn requests(&self) -> Vec<(PathBuf, PathBuf, String)> {
            self.requests
                .lock()
                .expect("requests lock poisoned")
                .clone()
        }
    }

    #[async_trait]
    impl RuntimePathConfigStore for Store {
        fn load_config(
            &self,
            _app_root: &Path,
        ) -> Result<Option<RuntimePathConfigInfo>, DomainError> {
            Ok(self.config.clone())
        }

        async fn request_data_root_change(
            &self,
            app_root: &Path,
            current_data_root: &Path,
            raw_target: &str,
        ) -> Result<(), DomainError> {
            self.requests.lock().expect("requests lock poisoned").push((
                app_root.to_path_buf(),
                current_data_root.to_path_buf(),
                raw_target.to_string(),
            ));
            Ok(())
        }
    }

    fn abs(name: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"C:\tauritavern-test\{name}"))
        } else {
            PathBuf::from(format!("/tauritavern-test/{name}"))
        }
    }

    fn service(store: Arc<Store>) -> RuntimePathsService {
        RuntimePathsService::new(
            RuntimePathsSnapshot {
                mode: RuntimeModeInfo::Standard,
                app_root: abs("app"),
                data_root: abs("current"),
            },
            store,
        )
    }

    #[test]
    fn get_runtime_paths_includes_config_state() {
        let configured = abs("configured");
        let store = Store::new().with_config(RuntimePathConfigInfo {
            data_root: configured.clone(),
            migration_pending: true,
            migration_error: Some("failed once".to_string()),
        });
        let service = service(store);

        let info = service.get_runtime_paths().unwrap();

        assert_eq!(info.mode, RuntimeModeInfo::Standard);
        assert_eq!(info.data_root, abs("current"));
        assert_eq!(info.configured_data_root, Some(configured));
        assert!(info.migration_pending);
        assert_eq!(info.migration_error, Some("failed once".to_string()));
    }

    #[tokio::test]
    async fn request_data_root_change_delegates_to_store() {
        let store = Store::new();
        let service = service(store.clone());

        service.request_data_root_change(" /target ").await.unwrap();

        assert_eq!(
            store.requests(),
            vec![(abs("app"), abs("current"), " /target ".to_string())]
        );
    }
}
