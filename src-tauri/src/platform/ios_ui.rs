#![cfg(target_os = "ios")]

use std::ffi::CStr;

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSArray;

use crate::domain::errors::DomainError;

use objc2_ui_kit::{UIViewController, UIWindow};

/// Resolve the top-most view controller for presenting modal UI.
///
/// Must be called on the main thread.
pub fn resolve_presenting_view_controller() -> Result<Retained<UIViewController>, DomainError> {
    let ui_application =
        AnyClass::get(unsafe { CStr::from_bytes_with_nul_unchecked(b"UIApplication\0") })
            .ok_or_else(|| {
                DomainError::InternalError("UIApplication class is unavailable".to_string())
            })?;

    let app: Option<Retained<AnyObject>> = unsafe { msg_send![ui_application, sharedApplication] };
    let app = app.ok_or_else(|| {
        DomainError::InternalError("UIApplication.sharedApplication returned null".to_string())
    })?;

    let windows: Option<Retained<NSArray<UIWindow>>> = unsafe { msg_send![&*app, windows] };
    let windows = windows.ok_or_else(|| {
        DomainError::InternalError("UIApplication.windows returned null".to_string())
    })?;

    let windows = windows.to_vec();
    if windows.is_empty() {
        return Err(DomainError::InternalError(
            "UIApplication.windows returned an empty list".to_string(),
        ));
    }

    let mut chosen_window = None;
    for window in windows {
        if chosen_window.is_none() {
            chosen_window = Some(window.clone());
        }

        if window.isKeyWindow() {
            chosen_window = Some(window);
            break;
        }
    }

    let ui_window = chosen_window.expect("windows list is non-empty");

    let mut current = ui_window.rootViewController().ok_or_else(|| {
        DomainError::InternalError("UIWindow.rootViewController returned null".to_string())
    })?;

    while let Some(next) = current.presentedViewController() {
        current = next;
    }

    Ok(current)
}
