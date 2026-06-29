use tauri::AppHandle;

use crate::application::services::bundled_template_service::BundledTemplateStore;
use crate::domain::errors::DomainError;
use crate::infrastructure::assets::read_resource_text;

#[derive(Clone)]
pub(crate) struct BundledResourceStore {
    app_handle: AppHandle,
}

impl BundledResourceStore {
    pub(crate) fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl BundledTemplateStore for BundledResourceStore {
    fn read_text(&self, relative_path: &str) -> Result<String, DomainError> {
        read_resource_text(&self.app_handle, relative_path)
    }
}
