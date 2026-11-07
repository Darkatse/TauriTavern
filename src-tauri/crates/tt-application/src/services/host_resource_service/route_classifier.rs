use crate::client_asset_paths::{
    THIRD_PARTY_EXTENSION_ROUTE_PREFIX, THUMBNAIL_ROUTE_PATH, USER_CSS_ROUTE,
    is_user_data_asset_route,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostResourceRoute {
    UserCss,
    ThirdPartyAsset,
    Thumbnail,
    UserDataAsset,
}

pub(crate) fn classify_host_resource_route(uri: &http::Uri) -> Option<HostResourceRoute> {
    let path = uri.path();
    if path == USER_CSS_ROUTE {
        return Some(HostResourceRoute::UserCss);
    }

    if path.starts_with(THIRD_PARTY_EXTENSION_ROUTE_PREFIX) {
        return Some(HostResourceRoute::ThirdPartyAsset);
    }

    if path == THUMBNAIL_ROUTE_PATH {
        return Some(HostResourceRoute::Thumbnail);
    }

    if is_user_data_asset_route(path) {
        return Some(HostResourceRoute::UserDataAsset);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Uri;

    fn classify_path(path: &'static str) -> Option<HostResourceRoute> {
        let uri: Uri = path.parse().expect("valid uri");
        classify_host_resource_route(&uri)
    }

    #[test]
    fn keeps_browser_resource_route_order() {
        assert_eq!(
            classify_path("/css/user.css"),
            Some(HostResourceRoute::UserCss)
        );
        assert_eq!(
            classify_path("/scripts/extensions/third-party/a/b.js"),
            Some(HostResourceRoute::ThirdPartyAsset)
        );
        assert_eq!(
            classify_path("/thumbnail"),
            Some(HostResourceRoute::Thumbnail)
        );
        assert_eq!(
            classify_path("/backgrounds/a.mp4"),
            Some(HostResourceRoute::UserDataAsset)
        );
        assert_eq!(classify_path("/index.html"), None);
    }
}
