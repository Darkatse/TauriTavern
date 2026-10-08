//! Delivery of host-served resources that the WebView downloads natively.
//!
//! The browser's native "Save image as" / "Save link as" pipeline fetches the
//! URL over the real network stack and bypasses `on_web_resource_request`
//! interception. Same-origin URLs that the host serves from user data
//! (images, backgrounds, avatars, assets, ...) do not exist on the network,
//! so native downloads of them fail — visibly as a `.txt` extension inferred
//! from the error body's content type.
//!
//! The native UX is kept intact: the user picks a location in the native save
//! dialog, then `DownloadStarting` reports the chosen path as the destination.
//! This module cancels the doomed network download and writes the host-served
//! bytes to that same location, correcting the extension that WebView2
//! guessed from its failed network probe. A staging + save-dialog flow stays
//! as the fallback for destinations that cannot be written directly.
//! External URLs keep the native pipeline untouched.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::webview::DownloadEvent;
use tauri::{Manager, Webview, Wry};
use url::Url;

use tt_application::services::host_resource_service::HostResourceService;

use super::tauri_resource_adapter::WRY_DELIVERY;
use crate::infrastructure::staging;
use crate::platform::file_transfer;

const DOWNLOAD_STAGING_KIND: &str = "download";
const FALLBACK_STAGING_EXTENSION: &str = "bin";
const FALLBACK_FILE_NAME: &str = "download.bin";

/// Handles one WebView download event.
///
/// Returns `true` when the native download pipeline may proceed, `false` when
/// this module takes over delivery of the bytes.
pub(crate) fn handle_download_event(
    host_resources: &Arc<HostResourceService>,
    webview: &Webview<Wry>,
    event: DownloadEvent<'_>,
) -> bool {
    let DownloadEvent::Requested {
        url, destination, ..
    } = event
    else {
        // Only the request decision controls the native pipeline.
        return true;
    };

    if !is_page_origin(webview, &url) {
        // External URLs keep the native download pipeline.
        return true;
    }

    let Some(request) = host_resource_request(&url) else {
        return true;
    };
    if !host_resources.serves_route(request.uri()) {
        // Same-origin but not a host-owned route (frontend asset, dev file).
        return true;
    }

    // The native pipeline cannot fetch this URL, so deny it and deliver the
    // host bytes to the location the user already picked; the callback must
    // not block on file IO.
    let chosen = destination.clone();
    let app = webview.app_handle().clone();
    let window_label = webview.label().to_string();
    let host_resources = Arc::clone(host_resources);
    tauri::async_runtime::spawn(async move {
        deliver_host_resource(host_resources, app, window_label, url, request, chosen).await;
    });
    false
}

/// Only the page's own origin is eligible for host delivery; this keeps the
/// browser's save semantics and prevents path collisions on external URLs.
fn is_page_origin(webview: &Webview<Wry>, url: &Url) -> bool {
    webview
        .url()
        .map(|page| page.origin() == url.origin())
        .unwrap_or(false)
}

fn host_resource_request(url: &Url) -> Option<tauri::http::Request<Vec<u8>>> {
    let path_and_query = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_string(),
    };
    tauri::http::Request::builder()
        .method(tauri::http::Method::GET)
        .uri(path_and_query)
        .body(Vec::new())
        .ok()
}

async fn deliver_host_resource(
    host_resources: Arc<HostResourceService>,
    app: tauri::AppHandle<Wry>,
    window_label: String,
    url: Url,
    request: tauri::http::Request<Vec<u8>>,
    chosen: PathBuf,
) {
    let served = tauri::async_runtime::spawn_blocking(move || {
        host_resources
            .try_serve(&request, WRY_DELIVERY)
            .map(|response| {
                let status = response.status();
                let content_type = response
                    .headers()
                    .get(tauri::http::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                (status, content_type, response.into_body())
            })
    })
    .await;

    let (status, content_type, bytes) = match served {
        Ok(Some(served)) => served,
        Ok(None) => {
            tracing::warn!(%url, "Download classified as host route but was not served");
            return;
        }
        Err(error) => {
            tracing::warn!(%url, %error, "Host resource download task failed");
            return;
        }
    };
    if !status.is_success() {
        tracing::warn!(%url, %status, "Host resource download target does not exist");
        return;
    }

    let chosen_url = url.clone();
    let chosen_content_type = content_type.clone();
    let written = tauri::async_runtime::spawn_blocking(move || {
        write_to_chosen_destination(&chosen, &chosen_url, &chosen_content_type, bytes)
    })
    .await;

    match written {
        Ok(Ok(path)) => {
            tracing::info!(%url, path = %path.display(), "Host resource download delivered");
        }
        Ok(Err((error, bytes))) => {
            tracing::warn!(%url, %error, "Failed to write host resource download; falling back to save dialog");
            deliver_via_staging(app, window_label, bytes, url, &content_type).await;
        }
        Err(error) => {
            tracing::warn!(%url, %error, "Host resource download task failed");
        }
    }
}

/// Writes the host bytes next to where the user already chose to save them.
///
/// WebView2 names the file from its failed network probe (typically `.txt`);
/// the URL or the served content type knows the real extension, so the file is
/// written under the corrected name without clobbering existing files.
fn write_to_chosen_destination(
    destination: &Path,
    url: &Url,
    content_type: &str,
    bytes: Vec<u8>,
) -> Result<PathBuf, (std::io::Error, Vec<u8>)> {
    if !destination.is_absolute() || destination.as_os_str().is_empty() {
        return Err((
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "The native dialog did not provide a usable destination",
            ),
            bytes,
        ));
    }

    let mut target = corrected_destination(destination, url, content_type);
    if target != destination && target.exists() {
        // The native dialog only checked its own suggested name; the corrected
        // name must not clobber an existing file.
        let parent = target.parent().unwrap_or(Path::new(""));
        let stem = target
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = target
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned());
        let mut index = 1;
        loop {
            let candidate = match &extension {
                Some(extension) => parent.join(format!("{stem} ({index}).{extension}")),
                None => parent.join(format!("{stem} ({index})")),
            };
            if !candidate.exists() {
                target = candidate;
                break;
            }
            index += 1;
        }
    }

    if let Err(error) = std::fs::write(&target, &bytes) {
        return Err((error, bytes));
    }
    // Flushing the buffers protects against loss on a close that follows
    // immediately after the save (mirrors the desktop delivery flow).
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&target) {
        let _ = file.sync_all();
    }
    Ok(target)
}

/// Returns `destination` with the extension that matches the real resource.
fn corrected_destination(destination: &Path, url: &Url, content_type: &str) -> PathBuf {
    let Some(real_extension) = url_extension(url)
        .or_else(|| mime_extension(content_type))
        .map(|extension| extension.to_ascii_lowercase())
    else {
        return destination.to_path_buf();
    };

    let current = destination
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());
    if current.as_deref() == Some(real_extension.as_str()) {
        return destination.to_path_buf();
    }

    let stem = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    match destination.parent() {
        Some(parent) => parent.join(format!("{stem}.{real_extension}")),
        None => PathBuf::from(format!("{stem}.{real_extension}")),
    }
}

async fn deliver_via_staging(
    app: tauri::AppHandle<Wry>,
    window_label: String,
    bytes: Vec<u8>,
    url: Url,
    content_type: &str,
) {
    let file_name = suggested_file_name(&url).unwrap_or_else(|| FALLBACK_FILE_NAME.to_string());
    let extension = url_extension(&url)
        .or_else(|| mime_extension(content_type))
        .unwrap_or_else(|| FALLBACK_STAGING_EXTENSION.to_owned());
    let staged = match staging::create_staged_file(&app, DOWNLOAD_STAGING_KIND, &extension).await {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(%error, "Failed to create staged download file");
            return;
        }
    };

    if let Err(error) = tokio::fs::write(&staged, &bytes).await {
        tracing::warn!(%error, "Failed to buffer download bytes");
        discard_staged(&staged).await;
        return;
    }

    let Some(window) = app.get_webview_window(&window_label) else {
        tracing::warn!(window = %window_label, "Download source window is gone");
        discard_staged(&staged).await;
        return;
    };

    // `deliver` owns the staged file from here, including on cancellation.
    if let Err(error) = file_transfer::deliver(&window, staged, &file_name).await {
        tracing::warn!(%error, "Failed to deliver host resource download");
    }
}

async fn discard_staged(staged: &Path) {
    if let Err(error) = staging::discard_file(staged).await {
        tracing::warn!(%error, "Failed to discard staged download file");
    }
}

fn suggested_file_name(url: &Url) -> Option<String> {
    let segment = url.path().rsplit('/').next()?;
    if segment.is_empty() {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .ok()?
        .into_owned();
    if decoded.is_empty() {
        return None;
    }
    Some(decoded)
}

fn file_extension(file_name: &str) -> Option<&str> {
    let candidate = file_name.rsplit_once('.')?.1;
    let valid = !candidate.is_empty()
        && candidate.len() <= 12
        && candidate.chars().all(|ch| ch.is_ascii_alphanumeric());
    valid.then_some(candidate)
}

fn url_extension(url: &Url) -> Option<String> {
    let name = suggested_file_name(url)?;
    file_extension(&name).map(str::to_owned)
}

/// Reverse map for the media types the host actually serves. `mime_guess`
/// only maps extensions to types, so downloads whose URL carries no extension
/// (e.g. `/thumbnail`) rely on this table; unknown types keep the dialog name.
fn mime_extension(content_type: &str) -> Option<String> {
    let essence = content_type.split(';').next()?.trim().to_ascii_lowercase();
    let extension = match essence.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/flac" => "flac",
        "text/plain" => "txt",
        "text/html" => "html",
        "text/css" => "css",
        "application/json" => "json",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        _ => return None,
    };
    Some(extension.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(segment: &str) -> Url {
        Url::parse(&format!(
            "tauri://localhost/user/images/%E8%8B%8D%E7%8E%84%E7%95%8C/{segment}"
        ))
        .expect("valid url")
    }

    #[test]
    fn suggests_decoded_file_name_from_url_path() {
        let name = suggested_file_name(&url("comfy_img-282ff941.png")).expect("file name");
        assert_eq!(name, "comfy_img-282ff941.png");
    }

    #[test]
    fn rejects_trailing_slash_urls_without_a_file_name() {
        let parsed = Url::parse("tauri://localhost/user/images/").expect("valid url");
        assert!(suggested_file_name(&parsed).is_none());
    }

    #[test]
    fn keeps_alphanumeric_extensions() {
        assert_eq!(file_extension("comfy_img-282ff941.png"), Some("png"));
        assert_eq!(file_extension(".png"), Some("png"));
        assert_eq!(file_extension("comfy_img-282ff941"), None);
        assert_eq!(file_extension("archive.tar.gz_invalid"), None);
        assert_eq!(file_extension("a.012345678901234"), None);
    }

    #[test]
    fn corrects_the_webview_guessed_extension() {
        let destination = Path::new("C:\\Downloads\\comfy_img-1.txt");
        let corrected = corrected_destination(destination, &url("comfy_img-1.png"), "text/plain");
        assert_eq!(corrected, PathBuf::from("C:\\Downloads\\comfy_img-1.png"));
    }

    #[test]
    fn keeps_destination_extension_when_it_already_matches() {
        let destination = Path::new("C:\\Downloads\\comfy_img-1.PNG");
        let corrected = corrected_destination(destination, &url("comfy_img-1.png"), "image/png");
        assert_eq!(corrected, destination);
    }

    #[test]
    fn appends_an_extension_to_extensionless_destinations() {
        let destination = Path::new("C:\\Downloads\\comfy_img-1");
        let corrected = corrected_destination(destination, &url("comfy_img-1.png"), "image/png");
        assert_eq!(corrected, PathBuf::from("C:\\Downloads\\comfy_img-1.png"));
    }

    #[test]
    fn falls_back_to_the_content_type_when_the_url_has_no_extension() {
        let parsed =
            Url::parse("tauri://localhost/thumbnail?type=avatar&file=a.png").expect("valid url");
        let destination = Path::new("C:\\Downloads\\thumbnail.txt");
        let corrected = corrected_destination(destination, &parsed, "image/jpeg");
        assert_eq!(corrected, PathBuf::from("C:\\Downloads\\thumbnail.jpg"));
    }

    #[test]
    fn leaves_the_destination_alone_without_any_known_extension() {
        let destination = Path::new("C:\\Downloads\\comfy_img-1.txt");
        let corrected =
            corrected_destination(destination, &url("comfy_img-1"), "application/octet-stream");
        assert_eq!(corrected, destination);
    }

    #[test]
    fn does_not_clobber_existing_files_with_the_corrected_name() {
        let directory = std::env::temp_dir().join(format!("tt-download-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let destination = directory.join("a.txt");

        let written =
            write_to_chosen_destination(&destination, &url("a.png"), "text/plain", b"png".to_vec())
                .expect("first write lands on the corrected name");
        let second = write_to_chosen_destination(
            &destination,
            &url("a.png"),
            "text/plain",
            b"again".to_vec(),
        )
        .expect("second write avoids the corrected name");

        let first_path = directory.join("a.png");
        assert_eq!(written, first_path);
        assert_eq!(second, directory.join("a (1).png"));
        assert_eq!(std::fs::read(first_path).expect("first content"), b"png");
        std::fs::remove_dir_all(&directory).expect("cleanup");
    }
}
