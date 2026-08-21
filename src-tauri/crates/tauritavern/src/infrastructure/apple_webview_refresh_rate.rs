#![cfg(any(target_os = "ios", target_os = "macos"))]

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{msg_send, sel};
use objc2_foundation::NSString;

const NATIVE_REFRESH_RATE_FEATURE: &str = "PreferPageRenderingUpdatesNear60FPSEnabled";

/// Lets WebKit render at the current display's native refresh rate.
///
/// # Safety
///
/// `wkwebview` must be a live `WKWebView` on the main thread.
pub(super) unsafe fn enable_native_refresh_rate(wkwebview: &AnyObject) {
    match unsafe { set_native_refresh_rate_preference(wkwebview) } {
        Ok(()) => tracing::info!("WKWebView native display refresh rate enabled"),
        Err(reason) => tracing::warn!(
            reason,
            "WKWebView native display refresh rate unavailable; using WebKit default"
        ),
    }
}

unsafe fn set_native_refresh_rate_preference(wkwebview: &AnyObject) -> Result<(), &'static str> {
    let configuration: Retained<AnyObject> = unsafe { msg_send![wkwebview, configuration] };
    let preferences: Retained<AnyObject> = unsafe { msg_send![&*configuration, preferences] };
    let preferences_class = preferences.class();

    let supports_features: Bool =
        unsafe { msg_send![preferences_class, respondsToSelector: sel!(_features)] };
    // Older supported WebKit versions expose the same feature objects through
    // the pre-16.4/13.3 experimental list.
    let features: Retained<AnyObject> = if supports_features.as_bool() {
        unsafe { msg_send![preferences_class, _features] }
    } else {
        unsafe { msg_send![preferences_class, _experimentalFeatures] }
    };
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
