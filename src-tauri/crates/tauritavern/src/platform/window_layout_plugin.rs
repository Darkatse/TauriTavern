//! Plugin transport shared by Android and OpenHarmony window owners.
use base64::Engine;
use tauri::{AppHandle, Manager};
use tt_adapter_media::window_backdrop::BackdropStrip;
use tt_contracts::window_layout::{WindowBackdropRequest, WindowSnapshot};
use tt_domain::errors::DomainError;

use super::window_layout::WallpaperRenderTarget;

struct WindowLayoutPlugin<R: tauri::Runtime>(tauri::plugin::PluginHandle<R>);

pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("window-layout")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let handle =
                api.register_android_plugin("com.tauritavern.client", "WindowLayoutPlugin")?;
            #[cfg(target_env = "ohos")]
            let handle = api.register_ohos_plugin()?;
            app.manage(WindowLayoutPlugin(handle));
            Ok(())
        })
        .build()
}

async fn call<T: serde::de::DeserializeOwned>(
    app: &AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<T, DomainError> {
    app.state::<WindowLayoutPlugin<tauri::Wry>>()
        .0
        .run_mobile_plugin_async(method, args)
        .await
        .map_err(|error| DomainError::InternalError(format!("Native window {method}: {error}")))
}

pub(super) async fn snapshot(app: &AppHandle) -> Result<WindowSnapshot, DomainError> {
    call(app, "snapshot", serde_json::json!({})).await
}

pub(super) async fn begin_backdrop(
    app: &AppHandle,
    request: &WindowBackdropRequest,
) -> Result<Option<WallpaperRenderTarget>, DomainError> {
    #[derive(serde::Deserialize)]
    struct Acceptance {
        target: Option<WallpaperRenderTarget>,
    }
    Ok(call::<Acceptance>(
        app,
        "beginBackdrop",
        serde_json::json!({
            "windowRevision": request.window_revision,
            "color": request.color,
            "replaceWallpaper": request.wallpaper.is_some(),
        }),
    )
    .await?
    .target)
}

pub(super) async fn apply_backdrop(
    app: &AppHandle,
    target: WallpaperRenderTarget,
    strips: Vec<BackdropStrip>,
) -> Result<(), DomainError> {
    let strips: Vec<_> = strips
        .into_iter()
        .map(|strip| {
            serde_json::json!({
                "x": strip.x, "y": strip.y,
                "png": base64::engine::general_purpose::STANDARD.encode(strip.png),
            })
        })
        .collect();
    call::<serde_json::Value>(app, "applyBackdrop", serde_json::json!({
        "windowRevision": target.window.revision, "wallpaperToken": target.token, "strips": strips,
    })).await?;
    Ok(())
}
