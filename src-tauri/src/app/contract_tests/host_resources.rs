use std::sync::Arc;

use tokio::fs;
use tt_adapter_media::FilesystemHostResourceStore;
use tt_contracts::host_resource::{
    HostResourceHeader, HostResourceHeaders, HostResourceMethod, HostResourceRequest,
    HostResourceResponse, header, status,
};

use super::temp_root;
use crate::application::services::host_resource_service::HostResourceService;

fn response_header<'a>(response: &'a HostResourceResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

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

    let range_headers = [HostResourceHeader {
        name: header::RANGE,
        value: b"bytes=1-2",
    }];
    let request = HostResourceRequest::new(
        HostResourceMethod::Get,
        "/backgrounds/a.mp4",
        None,
        HostResourceHeaders::new(&range_headers),
    );
    let response = service.try_serve(&request).expect("serve background range");

    assert_eq!(response.status, status::PARTIAL_CONTENT);
    assert_eq!(response.body, b"bc");
    assert_eq!(
        response_header(&response, header::CONTENT_RANGE),
        Some("bytes 1-2/4")
    );
    assert_eq!(
        response_header(&response, header::CONTENT_LENGTH),
        Some("2")
    );
    assert_eq!(
        response_header(&response, header::ACCEPT_RANGES),
        Some("bytes")
    );
    assert_eq!(
        response_header(&response, header::CONTENT_TYPE),
        Some("video/mp4")
    );

    let invalid_range_headers = [HostResourceHeader {
        name: header::RANGE,
        value: b"bytes=0-1,2-3",
    }];
    let request = HostResourceRequest::new(
        HostResourceMethod::Get,
        "/backgrounds/a.mp4",
        None,
        HostResourceHeaders::new(&invalid_range_headers),
    );
    let response = service.try_serve(&request).expect("serve invalid range");

    assert_eq!(response.status, status::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        response_header(&response, header::CONTENT_RANGE),
        Some("bytes */4")
    );
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
    let request = HostResourceRequest::new(
        HostResourceMethod::Get,
        "/thumbnail",
        Some("type=bg&file=a.gif"),
        HostResourceHeaders::empty(),
    );

    let response = service.try_serve(&request).expect("serve thumbnail");

    assert_eq!(response.status, status::OK);
    assert_eq!(response.body, b"gif");
    assert_eq!(
        response_header(&response, header::CONTENT_TYPE),
        Some("image/gif")
    );
}
