//! Host-owned browser resource services.
//!
//! These services make runtime data look like ordinary browser resources:
//! extension assets, thumbnails, user CSS, backgrounds, avatars, and upload
//! staging. Keep browser-visible path semantics here or in the presentation web
//! resource adapter; do not leak Tauri file APIs into frontend code.

use std::sync::Arc;

use crate::infrastructure::bundled_resources::BundledResourceStore;
use crate::infrastructure::paths::RuntimePaths;
use tauri::Manager;
use tt_adapter_media::{FilesystemHostResourceStore, FilesystemUserMediaStore};
use tt_application::services::bundled_template_service::BundledTemplateService;
use tt_application::services::host_resource_service::HostResourceService;
use tt_application::services::user_media_service::UserMediaService;
use tt_domain::models::settings::TauriTavernSettings;

pub(super) fn install_bundled_templates(app: &mut tauri::App, app_handle: &tauri::AppHandle) {
    // Template reads are command-driven and independent of the user data root.
    app.manage(Arc::new(BundledTemplateService::new(Arc::new(
        BundledResourceStore::new(app_handle.clone()),
    ))));
}

pub(super) fn install_runtime_resources(
    app: &mut tauri::App,
    runtime_paths: &RuntimePaths,
) -> Arc<HostResourceService> {
    // HostResourceService must be managed before the main window is created:
    // both production web resource interception and dev `tt-ext` serving read
    // the same state. Backend initialization applies the persisted settings;
    // avatar thumbnails are only requested after the backend is ready.
    let host_resource_store = Arc::new(FilesystemHostResourceStore::from_data_root(
        &runtime_paths.data_root,
    ));
    let host_resource_service = Arc::new(HostResourceService::new(
        TauriTavernSettings::default().avatar_persona_original_images_enabled,
        host_resource_store.clone(),
    ));
    app.manage(host_resource_service.clone());
    #[cfg(mobile)]
    app.manage(Arc::new(
        tt_adapter_media::window_backdrop::WindowBackdropRenderer::new(host_resource_store),
    ));

    // User media is a separate command-facing service, but it shares the same
    // runtime data root and should be published with the rest of resource state.
    let user_media_store = Arc::new(FilesystemUserMediaStore::from_data_root(
        &runtime_paths.data_root,
    ));
    app.manage(Arc::new(UserMediaService::new(user_media_store)));

    host_resource_service
}
