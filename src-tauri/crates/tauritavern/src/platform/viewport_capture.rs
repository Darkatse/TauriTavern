use tt_domain::errors::DomainError;

pub async fn capture_png(webview: &tauri::Webview) -> Result<Vec<u8>, DomainError> {
    #[cfg(target_os = "macos")]
    return crate::infrastructure::macos_webview_capture::capture_png(webview).await;
    #[cfg(target_os = "ios")]
    return crate::infrastructure::ios_webview_capture::capture_png(webview).await;
    #[cfg(target_os = "windows")]
    return crate::infrastructure::webview2_capture::capture_png(webview).await;
    #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
    return crate::infrastructure::webkitgtk_capture::capture_png(webview).await;
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    {
        use tauri::Manager;
        super::window_layout_plugin::capture_viewport(webview.app_handle()).await
    }
}
