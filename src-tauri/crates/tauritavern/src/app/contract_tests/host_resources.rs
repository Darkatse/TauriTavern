use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use tauri::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
    RANGE,
};
use tauri::http::{Method, Request, StatusCode};
use tokio::fs;
use tt_adapter_media::FilesystemHostResourceStore;

use super::temp_root;
use tt_application::services::host_resource_service::{
    HostResourceDeliveryCapabilities, HostResourceService,
};

const DELIVERY: HostResourceDeliveryCapabilities =
    HostResourceDeliveryCapabilities::new(true, false);

#[tokio::test]
async fn filesystem_host_resources_serve_background_video_range() {
    let root = temp_root("host-resource-range");
    let background = root.join("default-user/backgrounds/a.mp4");
    fs::create_dir_all(background.parent().expect("background parent"))
        .await
        .expect("create background dir");
    fs::write(&background, b"abcd")
        .await
        .expect("write background video");

    let service = HostResourceService::new(
        false,
        Arc::new(FilesystemHostResourceStore::from_data_root(&root)),
    );

    let request = Request::builder()
        .method(Method::GET)
        .uri("/backgrounds/a.mp4")
        .header(RANGE, "bytes=1-2")
        .body(Vec::new())
        .expect("range request");
    let response = service
        .try_serve(&request, DELIVERY)
        .expect("serve background range");

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.body(), b"bc");
    assert_eq!(response.headers()[CONTENT_RANGE], "bytes 1-2/4");
    assert_eq!(response.headers()[CONTENT_LENGTH], "2");
    assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
    assert_eq!(response.headers()[CONTENT_TYPE], "video/mp4");
    assert_eq!(response.headers()[CACHE_CONTROL], "private, no-cache");
    assert!(response.headers().contains_key(ETAG));

    let request = Request::builder()
        .method(Method::GET)
        .uri("/backgrounds/a.mp4")
        .header(RANGE, "bytes=0-1,2-3")
        .body(Vec::new())
        .expect("invalid range request");
    let response = service
        .try_serve(&request, DELIVERY)
        .expect("serve invalid range");

    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()[CONTENT_RANGE], "bytes */4");
}

#[tokio::test]
async fn filesystem_host_resources_return_original_for_animated_thumbnail() {
    let root = temp_root("host-resource-thumbnail");
    let background = root.join("default-user/backgrounds/a.gif");
    fs::create_dir_all(background.parent().expect("background parent"))
        .await
        .expect("create background dir");
    fs::write(&background, b"gif")
        .await
        .expect("write animated background");

    let service = HostResourceService::new(
        false,
        Arc::new(FilesystemHostResourceStore::from_data_root(&root)),
    );
    let request = Request::builder()
        .method(Method::GET)
        .uri("/thumbnail?type=bg&file=a.gif")
        .body(Vec::new())
        .expect("thumbnail request");

    let response = service
        .try_serve(&request, DELIVERY)
        .expect("serve thumbnail");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.body(), b"gif");
    assert_eq!(response.headers()[CONTENT_TYPE], "image/gif");
}

#[tokio::test]
async fn filesystem_host_resources_generate_required_static_animated_preview() {
    let root = temp_root("host-resource-static-animated-thumbnail");
    let background = root.join("default-user/backgrounds/a.gif");
    fs::create_dir_all(background.parent().expect("background parent"))
        .await
        .expect("create background dir");
    let gif = base64::engine::general_purpose::STANDARD
        .decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==")
        .expect("decode gif fixture");
    fs::write(&background, gif)
        .await
        .expect("write animated background");

    let service = HostResourceService::new(
        false,
        Arc::new(FilesystemHostResourceStore::from_data_root(&root)),
    );
    let request = Request::builder()
        .method(Method::GET)
        .uri("/thumbnail?type=bg&file=a.gif&static=true")
        .body(Vec::new())
        .expect("static thumbnail request");

    let response = service
        .try_serve(&request, DELIVERY)
        .expect("serve static thumbnail");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_TYPE], "image/jpeg");
    assert!(response.body().starts_with(&[0xff, 0xd8]));
    assert_eq!(response.headers()[CACHE_CONTROL], "private, no-cache");
    assert!(response.headers().contains_key(ETAG));
}

#[tokio::test]
async fn generated_thumbnail_revalidation_tracks_source_replacement() {
    let root = temp_root("host-resource-generated-thumbnail-revalidation");
    let background = root.join("default-user/backgrounds/a.png");
    fs::create_dir_all(background.parent().expect("background parent"))
        .await
        .expect("create background dir");
    image::ImageBuffer::from_pixel(2, 2, image::Rgba([255u8, 0, 0, 255]))
        .save(&background)
        .expect("write first image");
    let source_mtime = std::fs::metadata(&background)
        .expect("source metadata")
        .modified()
        .expect("source mtime");
    let service = HostResourceService::new(
        false,
        Arc::new(FilesystemHostResourceStore::from_data_root(&root)),
    );
    let request = Request::builder()
        .method(Method::GET)
        .uri("/thumbnail?type=bg&file=a.png")
        .body(Vec::new())
        .expect("initial thumbnail request");
    let initial = service.try_serve(&request, DELIVERY).expect("initial");
    let initial_etag = initial.headers()[ETAG].clone();

    image::ImageBuffer::from_pixel(2, 2, image::Rgba([0u8, 0, 255, 255]))
        .save(&background)
        .expect("replace image");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&background)
        .expect("open replacement image")
        .set_times(std::fs::FileTimes::new().set_modified(source_mtime + Duration::from_secs(1)))
        .expect("advance source mtime");

    let conditional = Request::builder()
        .method(Method::GET)
        .uri("/thumbnail?type=bg&file=a.png")
        .header(IF_NONE_MATCH, initial_etag.clone())
        .body(Vec::new())
        .expect("conditional thumbnail request");
    let changed = service.try_serve(&conditional, DELIVERY).expect("changed");

    assert_eq!(changed.status(), StatusCode::OK);
    assert_ne!(changed.headers()[ETAG], initial_etag);
    assert_eq!(changed.headers()[CONTENT_TYPE], "image/jpeg");
}

#[tokio::test]
async fn filesystem_host_resources_revalidate_without_reading_stale_content() {
    let root = temp_root("host-resource-revalidation");
    let background = root.join("default-user/backgrounds/a.png");
    fs::create_dir_all(background.parent().expect("background parent"))
        .await
        .expect("create background dir");
    fs::write(&background, b"old")
        .await
        .expect("write background");
    let service = HostResourceService::new(
        false,
        Arc::new(FilesystemHostResourceStore::from_data_root(&root)),
    );
    let request = Request::builder()
        .method(Method::GET)
        .uri("/backgrounds/a.png")
        .body(Vec::new())
        .expect("initial request");
    let initial = service.try_serve(&request, DELIVERY).expect("initial");
    let mut conditional = Request::builder()
        .method(Method::GET)
        .uri("/backgrounds/a.png")
        .body(Vec::new())
        .expect("conditional request");
    conditional
        .headers_mut()
        .insert(IF_NONE_MATCH, initial.headers()[ETAG].clone());

    let not_modified = service
        .try_serve(&conditional, DELIVERY)
        .expect("not modified");

    assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    assert!(not_modified.body().is_empty());
    assert!(!not_modified.headers().contains_key(CONTENT_TYPE));
}
