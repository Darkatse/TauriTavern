use std::{cell::RefCell, ffi::c_void};

use block2::RcBlock;
use objc2::{
    class, msg_send,
    rc::{Allocated, Retained},
    runtime::AnyObject,
};
use objc2_foundation::{NSError, NSPoint, NSRect, NSSize};
use tt_domain::errors::DomainError;

const NS_BITMAP_IMAGE_FILE_TYPE_PNG: usize = 4;

#[link(name = "ScreenCaptureKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(image: *const c_void) -> usize;
    fn CGImageCreateWithImageInRect(image: *const c_void, rect: NSRect) -> *const c_void;
    fn CGImageRelease(image: *const c_void);
}

pub async fn capture_png(webview: &tauri::Webview) -> Result<Vec<u8>, DomainError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview.with_webview(move |native| unsafe {
        let view = &*native.inner().cast::<AnyObject>();
        let window: *mut AnyObject = msg_send![view, window];
        let window_id: isize = msg_send![window, windowNumber];
        let window_frame: NSRect = msg_send![window, frame];
        let visible: NSRect = msg_send![view, visibleRect];
        let view_frame: NSRect = msg_send![view, convertRect: visible, toView: std::ptr::null::<AnyObject>()];
        let content: NSRect = msg_send![window, contentLayoutRect];
        let left = view_frame.origin.x.max(content.origin.x);
        let bottom = view_frame.origin.y.max(content.origin.y);
        let right = (view_frame.origin.x + view_frame.size.width).min(content.origin.x + content.size.width);
        let top = (view_frame.origin.y + view_frame.size.height).min(content.origin.y + content.size.height);
        if right <= left || top <= bottom {
            let _ = sender.send(Err(failure("WebView has no visible content viewport")));
            return;
        }
        // AppKit uses a bottom-left window origin; the captured image uses a top-left origin.
        let crop = NSRect::new(NSPoint::new(left, window_frame.size.height - top), NSSize::new(right - left, top - bottom));
        let sender = RefCell::new(Some(sender));
        let completion = RcBlock::new(move |content: *mut AnyObject, error: *mut NSError| {
            let Some(sender) = sender.borrow_mut().take() else { return; };
            if let Some(error) = error.as_ref() {
                let _ = sender.send(Err(failure(error.localizedDescription())));
                return;
            }
            let Some(content) = content.as_ref() else {
                let _ = sender.send(Err(failure("ScreenCaptureKit returned no current-process content")));
                return;
            };
            let windows: Retained<AnyObject> = msg_send![content, windows];
            let count: usize = msg_send![&*windows, count];
            for index in 0..count {
                let candidate: *mut AnyObject = msg_send![&*windows, objectAtIndex: index];
                let id: u32 = msg_send![candidate, windowID];
                if id == window_id as u32 {
                    capture_window(candidate, crop, sender);
                    return;
                }
            }
            let _ = sender.send(Err(failure("Current-process content did not include the WebView window")));
        });
        let _: () = msg_send![class!(SCShareableContent), getCurrentProcessShareableContentWithCompletionHandler: &*completion];
    }).map_err(failure)?;
    receiver.await.map_err(failure)?
}

unsafe fn capture_window(
    window: *mut AnyObject,
    crop: NSRect,
    sender: tokio::sync::oneshot::Sender<Result<Vec<u8>, DomainError>>,
) {
    unsafe {
        let filter: Allocated<AnyObject> = msg_send![class!(SCContentFilter), alloc];
        let filter: Retained<AnyObject> =
            msg_send![filter, initWithDesktopIndependentWindow: window];
        let rect: NSRect = msg_send![&*filter, contentRect];
        let scale: f32 = msg_send![&*filter, pointPixelScale];
        let config: Retained<AnyObject> = msg_send![class!(SCStreamConfiguration), new];
        let _: () =
            msg_send![&*config, setWidth: (rect.size.width * f64::from(scale)).round() as usize];
        let _: () =
            msg_send![&*config, setHeight: (rect.size.height * f64::from(scale)).round() as usize];
        let _: () = msg_send![&*config, setShowsCursor: false];
        let _: () = msg_send![&*config, setIgnoreShadowsSingleWindow: true];
        let sender = RefCell::new(Some(sender));
        let completion = RcBlock::new(move |image: *const c_void, error: *mut NSError| {
            let result = if let Some(error) = error.as_ref() {
                Err(failure(error.localizedDescription()))
            } else {
                crop_and_encode(image, crop, rect.size.width)
            };
            if let Some(sender) = sender.borrow_mut().take() {
                let _ = sender.send(result);
            }
        });
        let _: () = msg_send![class!(SCScreenshotManager), captureImageWithFilter: &*filter, configuration: &*config, completionHandler: &*completion];
    }
}

unsafe fn crop_and_encode(
    image: *const c_void,
    crop: NSRect,
    window_width: f64,
) -> Result<Vec<u8>, DomainError> {
    if image.is_null() {
        return Err(failure("ScreenCaptureKit returned no image"));
    }
    unsafe {
        let scale = CGImageGetWidth(image) as f64 / window_width;
        let rect = NSRect::new(
            NSPoint::new(
                (crop.origin.x * scale).round(),
                (crop.origin.y * scale).round(),
            ),
            NSSize::new(
                (crop.size.width * scale).round(),
                (crop.size.height * scale).round(),
            ),
        );
        let cropped = CGImageCreateWithImageInRect(image, rect);
        if cropped.is_null() {
            return Err(failure("Cannot crop the WebView content viewport"));
        }
        let bitmap: Allocated<AnyObject> = msg_send![class!(NSBitmapImageRep), alloc];
        let bitmap: Retained<AnyObject> = msg_send![bitmap, initWithCGImage: cropped];
        CGImageRelease(cropped);
        let properties: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
        let png: Option<Retained<AnyObject>> = msg_send![&*bitmap, representationUsingType: NS_BITMAP_IMAGE_FILE_TYPE_PNG, properties: &*properties];
        let png = png.ok_or_else(|| failure("Cannot encode content viewport PNG"))?;
        let length: usize = msg_send![&*png, length];
        let bytes: *const u8 = msg_send![&*png, bytes];
        Ok(std::slice::from_raw_parts(bytes, length).to_vec())
    }
}

fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("macOS viewport capture: {error}"))
}
