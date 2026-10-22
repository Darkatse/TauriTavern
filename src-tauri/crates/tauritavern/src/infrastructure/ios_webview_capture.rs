use std::cell::RefCell;

use block2::RcBlock;
use objc2::{msg_send, runtime::AnyObject};
use objc2_foundation::NSError;
use tt_domain::errors::DomainError;

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    fn UIImagePNGRepresentation(image: *mut AnyObject) -> *mut AnyObject;
}

pub async fn capture_png(webview: &tauri::Webview) -> Result<Vec<u8>, DomainError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview.with_webview(move |native| unsafe {
        let sender = RefCell::new(Some(sender));
        let completion = RcBlock::new(move |image: *mut AnyObject, error: *mut NSError| {
            let result = if let Some(error) = error.as_ref() {
                Err(failure(error.localizedDescription()))
            } else {
                encode_png(image)
            };
            if let Some(sender) = sender.borrow_mut().take() { let _ = sender.send(result); }
        });
        let view = &*native.inner().cast::<AnyObject>();
        let _: () = msg_send![view, takeSnapshotWithConfiguration: std::ptr::null::<AnyObject>(), completionHandler: &*completion];
    }).map_err(failure)?;
    receiver.await.map_err(failure)?
}

unsafe fn encode_png(image: *mut AnyObject) -> Result<Vec<u8>, DomainError> {
    if image.is_null() {
        return Err(failure("WKWebView returned no image"));
    }
    unsafe {
        let data = UIImagePNGRepresentation(image);
        if data.is_null() {
            return Err(failure("Cannot encode UIImage as PNG"));
        }
        let length: usize = msg_send![data, length];
        let bytes: *const u8 = msg_send![data, bytes];
        Ok(std::slice::from_raw_parts(bytes, length).to_vec())
    }
}

fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("iOS viewport capture: {error}"))
}
