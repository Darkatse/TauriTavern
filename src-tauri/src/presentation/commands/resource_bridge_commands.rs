use std::sync::Arc;

use tauri::State;

use crate::application::services::bundled_template_service::BundledTemplateService;
use crate::presentation::errors::CommandError;

/// Read a frontend template file from bundled resources.
#[tauri::command]
pub fn read_frontend_template(
    name: String,
    templates: State<'_, Arc<BundledTemplateService>>,
) -> Result<String, CommandError> {
    templates
        .read_frontend_template(&name)
        .map_err(CommandError::from)
}

/// Read a built-in extension template file from bundled resources.
#[tauri::command]
pub fn read_frontend_extension_template(
    extension: String,
    name: String,
    templates: State<'_, Arc<BundledTemplateService>>,
) -> Result<String, CommandError> {
    templates
        .read_frontend_extension_template(&extension, &name)
        .map_err(CommandError::from)
}
