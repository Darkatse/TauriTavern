#![cfg(target_os = "macos")]

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{msg_send, sel};
use objc2_foundation::NSString;
use tauri::WebviewWindow;

const NATIVE_REFRESH_RATE_FEATURE: &str = "PreferPageRenderingUpdatesNear60FPSEnabled";

pub fn configure_main_wkwebview(window: &WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| unsafe {
        let wkwebview_ptr = webview.inner();
        assert!(
            !wkwebview_ptr.is_null(),
            "PlatformWebview.inner() returned a null WKWebView pointer"
        );

        let wkwebview = &*wkwebview_ptr.cast::<AnyObject>();
        match enable_native_refresh_rate(wkwebview) {
            Ok(()) => tracing::info!("WKWebView native display refresh rate enabled"),
            Err(reason) => tracing::warn!(
                reason,
                "WKWebView native display refresh rate unavailable; using WebKit default"
            ),
        }
        super::apple_webview_js_dialogs::install_js_dialog_ui_delegate(wkwebview);
    })
}

/// Lets WebKit render at the current display's native refresh rate.
///
/// # Safety
///
/// `wkwebview` must be a live `WKWebView` on the main thread.
unsafe fn enable_native_refresh_rate(wkwebview: &AnyObject) -> Result<(), &'static str> {
    let configuration: Retained<AnyObject> = unsafe { msg_send![wkwebview, configuration] };
    let preferences: Retained<AnyObject> = unsafe { msg_send![&*configuration, preferences] };
    let preferences_class = preferences.class();

    let supports_features: Bool =
        unsafe { msg_send![preferences_class, respondsToSelector: sel!(_features)] };
    if !supports_features.as_bool() {
        return Err("WKPreferences._features is unavailable");
    }

    let features: Retained<AnyObject> = unsafe { msg_send![preferences_class, _features] };
    let feature_count: usize = unsafe { msg_send![&*features, count] };
    let target_key = NSString::from_str(NATIVE_REFRESH_RATE_FEATURE);

    for index in 0..feature_count {
        let feature: Retained<AnyObject> = unsafe { msg_send![&*features, objectAtIndex: index] };
        let key: Retained<NSString> = unsafe { msg_send![&*feature, key] };
        if !key.isEqualToString(&target_key) {
            continue;
        }

        let _: () =
            unsafe { msg_send![&*preferences, _setEnabled: Bool::NO, forFeature: &*feature] };
        let still_capped: Bool =
            unsafe { msg_send![&*preferences, _isEnabledForFeature: &*feature] };

        return (!still_capped.as_bool())
            .then_some(())
            .ok_or("WebKit rejected the native refresh-rate preference");
    }

    Err("WebKit native refresh-rate feature is unavailable")
}
