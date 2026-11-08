mod adapters;
mod environment;
mod repositories;
mod services;

use std::path::Path;
use std::sync::Arc;

use tt_adapter_media::FilesystemHostResourceStore;
use tt_adapter_storage_core::file_system::DataDirectory;
use tt_adapter_sync::SyncAutomationStore;
use tt_application::services::safe_mode_service::SafeModeService;
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

pub(super) async fn safe_mode_service(
    environment: &HostEnvironment,
    data_root: &Path,
) -> Result<SafeModeService, DomainError> {
    let data_directory = DataDirectory::new(data_root.to_path_buf());
    let bundled_resources = environment.bundled_resources.clone();
    let factory_user_css = tokio::task::spawn_blocking(move || {
        bundled_resources.read_bytes("default/content/user.css")
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!("Default user.css load task failed: {error}"))
    })??;
    let settings_repository =
        repositories::settings_repository(environment, &data_directory).await?;
    repositories::prepare_user_data(&settings_repository, &data_directory).await?;

    Ok(SafeModeService::new(
        settings_repository,
        repositories::avatar_repository(&data_directory),
        repositories::extension_repository(environment, &data_directory),
        Arc::new(FilesystemHostResourceStore::from_data_root(data_root)),
        Arc::new(SyncAutomationStore::new(
            data_directory.default_user().to_path_buf(),
        )),
        factory_user_css,
    ))
}
