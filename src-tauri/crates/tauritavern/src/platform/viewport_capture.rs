use tt_domain::errors::DomainError;

pub async fn capture_png(webview: &tauri::Webview) -> Result<Vec<u8>, DomainError> {
    #[cfg(target_os = "macos")]
    let (platform, result) = (
        "macOS",
        crate::infrastructure::macos_webview_capture::capture_png(webview).await,
    );
    #[cfg(target_os = "ios")]
    let (platform, result) = (
        "iOS",
        crate::infrastructure::ios_webview_capture::capture_png(webview).await,
    );
    #[cfg(target_os = "windows")]
    let (platform, result) = (
        "Windows",
        crate::infrastructure::webview2_capture::capture_png(webview).await,
    );
    #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
    let (platform, result) = (
        "Linux",
        crate::infrastructure::webkitgtk_capture::capture_png(webview).await,
    );
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    let (platform, result) = {
        use tauri::Manager;
        #[cfg(target_os = "android")]
        let platform = "Android";
        #[cfg(target_env = "ohos")]
        let platform = "OpenHarmony";
        (
            platform,
            super::window_layout_plugin::capture_viewport(webview.app_handle()).await,
        )
    };
    result.map_err(|error| {
        DomainError::InternalError(format!("{platform} viewport capture: {error}"))
    })
}
