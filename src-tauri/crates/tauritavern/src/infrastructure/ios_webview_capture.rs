use std::cell::RefCell;

use block2::RcBlock;
use objc2::{msg_send, runtime::AnyObject};
use objc2_foundation::NSError;
use objc2_ui_kit::UIImage;

pub async fn capture_png(webview: &tauri::Webview) -> Result<Vec<u8>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview.with_webview(move |native| unsafe {
        let sender = RefCell::new(Some(sender));
        let completion = RcBlock::new(move |image: *mut UIImage, error: *mut NSError| {
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

unsafe fn encode_png(image: *mut UIImage) -> Result<Vec<u8>, String> {
    let image = unsafe { image.as_ref() }.ok_or_else(|| failure("WKWebView returned no image"))?;
    let data = image
        .png_representation()
        .ok_or_else(|| failure("Cannot encode UIImage as PNG"))?;
    Ok(data.to_vec())
}

fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}
