mod adapters;
mod repositories;
mod services;

use std::path::Path;

use tauri::AppHandle;

use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::file_system::DataDirectory;

use super::{AppServices, StartupProfile};

pub(super) async fn initialize_data_directory(
    data_root: &Path,
) -> Result<DataDirectory, DomainError> {
    let data_directory = DataDirectory::new(data_root.to_path_buf());
    data_directory.initialize().await?;
    Ok(data_directory)
}

pub(super) async fn build_services(
    app_handle: &AppHandle,
    data_directory: &DataDirectory,
    startup_profile: &StartupProfile,
) -> Result<AppServices, DomainError> {
    services::build(app_handle, data_directory, startup_profile).await
}
