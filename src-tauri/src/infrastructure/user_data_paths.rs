use std::path::PathBuf;

pub(crate) const CHARACTERS_ROUTE_PREFIX: &str = "/characters/";
pub(crate) const USER_AVATARS_ROUTE_PREFIX: &str = "/User Avatars/";
pub(crate) const USER_AVATARS_ROUTE_PREFIX_ENCODED: &str = "/User%20Avatars/";
pub(crate) const BACKGROUNDS_ROUTE_PREFIX: &str = "/backgrounds/";
pub(crate) const ASSETS_ROUTE_PREFIX: &str = "/assets/";
pub(crate) const USER_IMAGES_ROUTE_PREFIX: &str = "/user/images/";
pub(crate) const USER_FILES_ROUTE_PREFIX: &str = "/user/files/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UserDataAssetKind {
    Character,
    Persona,
    Background,
    Asset,
    UserImage,
    UserFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserDataAssetRequestPath {
    pub(crate) kind: UserDataAssetKind,
    pub(crate) relative_path: PathBuf,
    pub(crate) relative_path_display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UserDataPathError {
    MissingAssetPath,
    InvalidPath,
}

pub(crate) fn is_user_data_asset_route(path: &str) -> bool {
    path.starts_with(CHARACTERS_ROUTE_PREFIX)
        || path.starts_with(USER_AVATARS_ROUTE_PREFIX_ENCODED)
        || path.starts_with(USER_AVATARS_ROUTE_PREFIX)
        || path.starts_with(BACKGROUNDS_ROUTE_PREFIX)
        || path.starts_with(ASSETS_ROUTE_PREFIX)
        || path.starts_with(USER_IMAGES_ROUTE_PREFIX)
        || path.starts_with(USER_FILES_ROUTE_PREFIX)
}

pub(crate) fn parse_user_data_asset_request_path(
    path: &str,
) -> Result<Option<UserDataAssetRequestPath>, UserDataPathError> {
    let (kind, suffix) = if let Some(suffix) = path.strip_prefix(CHARACTERS_ROUTE_PREFIX) {
        (UserDataAssetKind::Character, suffix)
    } else if let Some(suffix) = path.strip_prefix(USER_AVATARS_ROUTE_PREFIX_ENCODED) {
        (UserDataAssetKind::Persona, suffix)
    } else if let Some(suffix) = path.strip_prefix(USER_AVATARS_ROUTE_PREFIX) {
        (UserDataAssetKind::Persona, suffix)
    } else if let Some(suffix) = path.strip_prefix(BACKGROUNDS_ROUTE_PREFIX) {
        (UserDataAssetKind::Background, suffix)
    } else if let Some(suffix) = path.strip_prefix(ASSETS_ROUTE_PREFIX) {
        (UserDataAssetKind::Asset, suffix)
    } else if let Some(suffix) = path.strip_prefix(USER_IMAGES_ROUTE_PREFIX) {
        (UserDataAssetKind::UserImage, suffix)
    } else if let Some(suffix) = path.strip_prefix(USER_FILES_ROUTE_PREFIX) {
        (UserDataAssetKind::UserFile, suffix)
    } else {
        return Ok(None);
    };

    let mut relative_segments = Vec::new();
    for raw_segment in suffix.split('/') {
        if raw_segment.is_empty() {
            continue;
        }

        let segment = crate::infrastructure::request_path::decode_request_segment(raw_segment)
            .map_err(|_| UserDataPathError::InvalidPath)?;

        if !crate::infrastructure::request_path::validate_path_segment(&segment) {
            return Err(UserDataPathError::InvalidPath);
        }

        relative_segments.push(segment);
    }

    if relative_segments.is_empty() {
        return Err(UserDataPathError::MissingAssetPath);
    }

    let mut relative_path = PathBuf::new();
    for segment in &relative_segments {
        relative_path.push(segment);
    }

    Ok(Some(UserDataAssetRequestPath {
        kind,
        relative_path,
        relative_path_display: relative_segments.join("/"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_character_asset_path() {
        let path = "/characters/avatar.png";
        let parsed = parse_user_data_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.kind, UserDataAssetKind::Character);
        assert_eq!(parsed.relative_path, PathBuf::from("avatar.png"));
        assert_eq!(parsed.relative_path_display, "avatar.png");
    }

    #[test]
    fn parses_persona_asset_path_with_encoded_prefix() {
        let path = "/User%20Avatars/me.png";
        let parsed = parse_user_data_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.kind, UserDataAssetKind::Persona);
        assert_eq!(parsed.relative_path, PathBuf::from("me.png"));
        assert_eq!(parsed.relative_path_display, "me.png");
    }

    #[test]
    fn normalizes_redundant_relative_separators() {
        let path = "/characters//nested//avatar.png";
        let parsed = parse_user_data_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(
            parsed.relative_path,
            PathBuf::from("nested").join("avatar.png")
        );
        assert_eq!(parsed.relative_path_display, "nested/avatar.png");
    }

    #[test]
    fn rejects_dot_segments() {
        let path = "/characters/../avatar.png";
        let result = parse_user_data_asset_request_path(path);
        assert_eq!(result, Err(UserDataPathError::InvalidPath));
    }

    #[test]
    fn rejects_encoded_path_separators() {
        let path = "/characters/%2fsecret.png";
        let result = parse_user_data_asset_request_path(path);
        assert_eq!(result, Err(UserDataPathError::InvalidPath));
    }

    #[test]
    fn parses_background_asset_path() {
        let path = "/backgrounds/space%20cat.png";
        let parsed = parse_user_data_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.kind, UserDataAssetKind::Background);
        assert_eq!(parsed.relative_path, PathBuf::from("space cat.png"));
        assert_eq!(parsed.relative_path_display, "space cat.png");
    }

    #[test]
    fn parses_nested_user_image_asset_path() {
        let path = "/user/images/folders/a.png";
        let parsed = parse_user_data_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.kind, UserDataAssetKind::UserImage);
        assert_eq!(parsed.relative_path, PathBuf::from("folders").join("a.png"));
        assert_eq!(parsed.relative_path_display, "folders/a.png");
    }
}
