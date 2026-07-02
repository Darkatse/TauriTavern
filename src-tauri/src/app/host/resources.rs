//! Host-owned browser resource services.
//!
//! These services make runtime data look like ordinary browser resources:
//! extension assets, thumbnails, user CSS, backgrounds, avatars, and upload
//! staging. Keep browser-visible path semantics here or in the presentation web
//! resource adapter; do not leak Tauri file APIs into frontend code.

use std::sync::Arc;

use crate::app::StartupProfile;
use crate::application::services::bundled_template_service::BundledTemplateService;
use crate::application::services::host_resource_service::HostResourceService;
use crate::application::services::user_media_service::UserMediaService;
use crate::infrastructure::bundled_resources::BundledResourceStore;
use crate::infrastructure::paths::RuntimePaths;
use tauri::Manager;
use tt_adapter_media::{FilesystemHostResourceStore, FilesystemUserMediaStore};

pub(super) fn install_bundled_templates(app: &mut tauri::App, app_handle: &tauri::AppHandle) {
    // Template reads are command-driven and independent of user data root, so the
    // bundled store can be managed before StartupProfile is loaded.
    app.manage(Arc::new(BundledTemplateService::new(Arc::new(
        BundledResourceStore::new(app_handle.clone()),
    ))));
}

pub(super) fn install_runtime_resources(
    app: &mut tauri::App,
    app_handle: &tauri::AppHandle,
    runtime_paths: &RuntimePaths,
    startup_profile: &StartupProfile,
) -> Arc<HostResourceService> {
    // tauri.conf.json whitelists standard Tauri directories; this dynamic scope
    // adds the resolved data root, including portable/migrated roots.
    if let Err(error) = app_handle
        .asset_protocol_scope()
        .allow_directory(&runtime_paths.data_root, true)
    {
        tracing::warn!(
            "Failed to extend asset protocol scope for {:?}: {}",
            runtime_paths.data_root,
            error
        );
    }

    // HostResourceService must be managed before the main window is created:
    // both production web resource interception and dev `tt-ext` serving read
    // the same state.
    let host_resource_store = Arc::new(FilesystemHostResourceStore::from_data_root(
        &runtime_paths.data_root,
    ));
    let host_resource_service = Arc::new(HostResourceService::new(
        startup_profile
            .tauritavern_settings
            .avatar_persona_original_images_enabled,
        host_resource_store,
    ));
    app.manage(host_resource_service.clone());

    // User media is a separate command-facing service, but it shares the same
    // runtime data root and should be published with the rest of resource state.
    let user_media_store = Arc::new(FilesystemUserMediaStore::from_data_root(
        &runtime_paths.data_root,
    ));
    app.manage(Arc::new(UserMediaService::new(user_media_store)));

    host_resource_service
}
