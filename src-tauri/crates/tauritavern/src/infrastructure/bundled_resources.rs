use tauri::AppHandle;

use crate::infrastructure::assets::{BundledResources, read_resource_bytes, read_resource_text};
use tt_domain::errors::DomainError;
use tt_ports::bundled_template::BundledTemplateStore;

#[derive(Clone)]
pub(crate) struct BundledResourceStore {
    app_handle: AppHandle,
}

impl BundledResourceStore {
    pub(crate) fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl BundledResources for BundledResourceStore {
    fn read_bytes(&self, relative_path: &str) -> Result<Vec<u8>, DomainError> {
        read_resource_bytes(&self.app_handle, relative_path)
    }
}

impl BundledTemplateStore for BundledResourceStore {
    fn read_text(&self, relative_path: &str) -> Result<String, DomainError> {
        read_resource_text(self, relative_path)
    }
}
