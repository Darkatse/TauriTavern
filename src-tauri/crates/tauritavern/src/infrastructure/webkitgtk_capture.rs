use tt_domain::errors::DomainError;
use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};

pub async fn capture_png(webview: &tauri::Webview) -> Result<Vec<u8>, DomainError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |native| {
            native.inner().snapshot(
                SnapshotRegion::Visible,
                SnapshotOptions::NONE,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    let result = result.map_err(failure).and_then(|surface| {
                        let image = cairo::ImageSurface::try_from(surface)
                            .map_err(|_| failure("WebKit returned a non-image snapshot surface"))?;
                        let mut png = Vec::new();
                        image.write_to_png(&mut png).map_err(failure)?;
                        Ok(png)
                    });
                    let _ = sender.send(result);
                },
            );
        })
        .map_err(failure)?;
    receiver.await.map_err(failure)?
}

fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Linux viewport capture: {error}"))
}
