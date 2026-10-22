//! Mobile window policy and wallpaper publication. Image IO stays in tt-adapter-media.
use tauri::AppHandle;
#[cfg(mobile)]
use tauri::Manager;
use tt_contracts::window_layout::{WindowBackdropRequest, WindowSnapshot};
use tt_domain::errors::DomainError;

#[cfg(target_os = "ios")]
use super::ios_window_layout as native;
#[cfg(any(target_os = "android", target_env = "ohos"))]
use super::window_layout_plugin as native;
#[cfg(any(target_os = "android", target_env = "ohos"))]
pub use super::window_layout_plugin::plugin;

/// Native acceptance binds an image render to the actual window and current wallpaper.
#[cfg(mobile)]
#[derive(serde::Deserialize)]
pub(super) struct WallpaperRenderTarget {
    pub window: WindowSnapshot,
    pub token: u64,
}

pub async fn snapshot(app: &AppHandle) -> Result<Option<WindowSnapshot>, DomainError> {
    #[cfg(mobile)]
    return native::snapshot(app).await.map(Some);
    #[cfg(desktop)]
    {
        let _ = app;
        Ok(None)
    }
}

pub async fn set_backdrop(
    app: &AppHandle,
    request: WindowBackdropRequest,
) -> Result<(), DomainError> {
    #[cfg(mobile)]
    {
        // Accept before waiting for the decoder: a new wallpaper invalidates old work now.
        let Some(target) = native::begin_backdrop(app, &request).await? else {
            // The window changed after the page took its snapshot; its next publication wins.
            return Ok(());
        };
        let Some(wallpaper) = request.wallpaper else {
            // Color does not affect transparent strips or an image render already in flight.
            return Ok(());
        };
        let renderer = app
            .state::<std::sync::Arc<tt_adapter_media::window_backdrop::WindowBackdropRenderer>>();
        let strips = renderer.render(target.window.clone(), wallpaper).await?;
        native::apply_backdrop(app, target, strips).await
    }
    #[cfg(desktop)]
    {
        let _ = (app, request);
        Err(DomainError::InvalidData(
            "Native window backdrops are available on mobile hosts".into(),
        ))
    }
}
