//! Mobile window policy and wallpaper publication. Image IO stays in tt-adapter-media.
use tauri::AppHandle;
// OHOS adopts this mobile policy in the next platform slice.
#[cfg(all(mobile, not(target_env = "ohos")))]
use tauri::Manager;
use tt_contracts::window_layout::{WindowBackdropRequest, WindowSnapshot};
use tt_domain::errors::DomainError;

#[cfg(target_os = "android")]
use super::android_window_layout as native;
#[cfg(target_os = "android")]
pub use super::android_window_layout::plugin;
#[cfg(target_os = "ios")]
use super::ios_window_layout as native;

/// Native acceptance binds an image render to the actual window and current wallpaper.
#[cfg(all(mobile, not(target_env = "ohos")))]
#[derive(serde::Deserialize)]
pub(super) struct WallpaperRenderTarget {
    pub window: WindowSnapshot,
    pub token: u64,
}

pub async fn snapshot(app: &AppHandle) -> Result<Option<WindowSnapshot>, DomainError> {
    #[cfg(all(mobile, not(target_env = "ohos")))]
    return native::snapshot(app).await.map(Some);
    #[cfg(any(desktop, target_env = "ohos"))]
    {
        let _ = app;
        Ok(None)
    }
}

pub async fn set_backdrop(
    app: &AppHandle,
    request: WindowBackdropRequest,
) -> Result<(), DomainError> {
    #[cfg(all(mobile, not(target_env = "ohos")))]
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
    #[cfg(any(desktop, target_env = "ohos"))]
    {
        let _ = (app, request);
        Err(DomainError::InvalidData(
            "Native window backdrops are available on Android and iOS".into(),
        ))
    }
}
