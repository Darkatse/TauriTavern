use std::sync::Arc;

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
