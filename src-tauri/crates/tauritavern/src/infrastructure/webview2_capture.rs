use std::{cell::RefCell, rc::Rc};

use tt_domain::errors::DomainError;
use webview2_com::{
    CapturePreviewCompletedHandler,
    Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
};
use windows::Win32::{
    Foundation::HGLOBAL,
    System::Com::{
        IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
        StructuredStorage::CreateStreamOnHGlobal,
    },
};

pub async fn capture_png(webview: &tauri::Webview) -> Result<Vec<u8>, DomainError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |native| {
            let sender = Rc::new(RefCell::new(Some(sender)));
            let result = (|| unsafe {
                let view = native.controller().CoreWebView2().map_err(failure)?;
                let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true).map_err(failure)?;
                let captured_stream = stream.clone();
                let completion_sender = sender.clone();
                let completion = CapturePreviewCompletedHandler::create(Box::new(move |status| {
                    let result = status
                        .map_err(failure)
                        .and_then(|()| read_stream(&captured_stream));
                    if let Some(sender) = completion_sender.borrow_mut().take() {
                        let _ = sender.send(result);
                    }
                    Ok(())
                }));
                view.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &stream,
                    &completion,
                )
                .map_err(failure)
            })();
            if let Err(error) = result {
                if let Some(sender) = sender.borrow_mut().take() {
                    let _ = sender.send(Err(error));
                }
            }
        })
        .map_err(failure)?;
    receiver.await.map_err(failure)?
}

unsafe fn read_stream(stream: &IStream) -> Result<Vec<u8>, DomainError> {
    unsafe {
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).map_err(failure)?;
        let length = u32::try_from(stat.cbSize).map_err(failure)?;
        stream.Seek(0, STREAM_SEEK_SET, None).map_err(failure)?;
        let mut bytes = vec![0_u8; length as usize];
        let mut read = 0;
        stream
            .Read(bytes.as_mut_ptr().cast(), length, Some(&mut read))
            .ok()
            .map_err(failure)?;
        if read != length {
            return Err(failure(format!(
                "PNG stream returned {read} of {length} bytes"
            )));
        }
        Ok(bytes)
    }
}

fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Windows viewport capture: {error}"))
}
