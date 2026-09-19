#![cfg(target_os = "macos")]

use objc2::runtime::AnyObject;
use tauri::WebviewWindow;

pub fn configure_main_wkwebview(window: &WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| unsafe {
        let wkwebview_ptr = webview.inner();
        assert!(
            !wkwebview_ptr.is_null(),
            "PlatformWebview.inner() returned a null WKWebView pointer"
        );

        let wkwebview = &*wkwebview_ptr.cast::<AnyObject>();
        super::apple_webview_refresh_rate::enable_native_refresh_rate(wkwebview);
        super::apple_webview_js_dialogs::install_js_dialog_ui_delegate(wkwebview);
    })
}
