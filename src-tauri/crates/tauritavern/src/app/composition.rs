mod adapters;
mod environment;
mod repositories;
mod services;

use std::path::Path;

use tt_adapter_storage_core::file_system::DataDirectory;
use tt_domain::errors::DomainError;

use super::{AppServices, StartupProfile};

pub(crate) use environment::HostEnvironment;

pub(super) async fn initialize_data_directory(
    data_root: &Path,
) -> Result<DataDirectory, DomainError> {
    let data_directory = DataDirectory::new(data_root.to_path_buf());
    data_directory.initialize().await?;
    Ok(data_directory)
}

pub(super) async fn build_services(
    environment: &HostEnvironment,
    app_root: &Path,
    data_directory: &DataDirectory,
    startup_profile: &StartupProfile,
) -> Result<AppServices, DomainError> {
    services::build(environment, app_root, data_directory, startup_profile).await
}
