use std::path::PathBuf;

pub const CHARACTERS_ROUTE_PREFIX: &str = "/characters/";
pub const USER_AVATARS_ROUTE_PREFIX: &str = "/User Avatars/";
pub const USER_AVATARS_ROUTE_PREFIX_ENCODED: &str = "/User%20Avatars/";
pub const BACKGROUNDS_ROUTE_PREFIX: &str = "/backgrounds/";
pub const ASSETS_ROUTE_PREFIX: &str = "/assets/";
pub const USER_IMAGES_ROUTE_PREFIX: &str = "/user/images/";
pub const USER_FILES_ROUTE_PREFIX: &str = "/user/files/";
pub const THIRD_PARTY_EXTENSION_ROUTE_PREFIX: &str = "/scripts/extensions/third-party/";
pub const USER_CSS_ROUTE: &str = "/css/user.css";
pub const THUMBNAIL_ROUTE_PATH: &str = "/thumbnail";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserDataAssetKind {
    Character,
    Persona,
    Background,
    Asset,
    UserImage,
    UserFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDataAssetRequestPath {
    pub kind: UserDataAssetKind,
    pub relative_path: PathBuf,
    pub relative_path_display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserDataPathError {
    MissingAssetPath,
    InvalidPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThirdPartyAssetRequestPath {
    pub extension_folder: String,
    pub relative_path: PathBuf,
    pub relative_path_display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThirdPartyPathError {
    MissingExtension,
    MissingAssetPath,
    InvalidPath,
}

pub fn is_user_data_asset_route(path: &str) -> bool {
    path.starts_with(CHARACTERS_ROUTE_PREFIX)
        || path.starts_with(USER_AVATARS_ROUTE_PREFIX_ENCODED)
        || path.starts_with(USER_AVATARS_ROUTE_PREFIX)
        || path.starts_with(BACKGROUNDS_ROUTE_PREFIX)
        || path.starts_with(ASSETS_ROUTE_PREFIX)
        || path.starts_with(USER_IMAGES_ROUTE_PREFIX)
        || path.starts_with(USER_FILES_ROUTE_PREFIX)
}

pub fn parse_user_data_asset_request_path(
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

        let segment =
            decode_request_segment(raw_segment).map_err(|_| UserDataPathError::InvalidPath)?;

        if !validate_path_segment(&segment) {
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

pub fn parse_third_party_asset_request_path(
    path: &str,
) -> Result<Option<ThirdPartyAssetRequestPath>, ThirdPartyPathError> {
    let suffix = match path.strip_prefix(THIRD_PARTY_EXTENSION_ROUTE_PREFIX) {
        Some(value) => value,
        None => return Ok(None),
    };

    let mut raw_segments = suffix.split('/');
    let extension_folder = decode_third_party_segment(
        raw_segments
            .next()
            .ok_or(ThirdPartyPathError::MissingExtension)?,
    )?;
    validate_third_party_segment(&extension_folder)?;

    let mut relative_segments = Vec::new();
    for raw_segment in raw_segments {
        if raw_segment.is_empty() {
            continue;
        }

        let segment = decode_third_party_segment(raw_segment)?;
        validate_third_party_segment(&segment)?;
        relative_segments.push(segment);
    }

    if relative_segments.is_empty() {
        return Err(ThirdPartyPathError::MissingAssetPath);
    }

    let mut relative_path = PathBuf::new();
    for segment in &relative_segments {
        relative_path.push(segment);
    }

    Ok(Some(ThirdPartyAssetRequestPath {
        extension_folder,
        relative_path,
        relative_path_display: relative_segments.join("/"),
    }))
}

fn decode_third_party_segment(segment: &str) -> Result<String, ThirdPartyPathError> {
    decode_request_segment(segment).map_err(|_| ThirdPartyPathError::InvalidPath)
}

fn decode_request_segment(segment: &str) -> Result<String, ()> {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| ())
}

fn is_forbidden_path_segment_char(character: char) -> bool {
    matches!(
        character,
        '\u{0000}'..='\u{001F}' | '\u{007F}' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
    )
}

/// Validate a decoded browser asset path segment.
///
/// C1 controls are intentionally allowed for legacy mojibake filenames from
/// migrated SillyTavern data. C0 controls and DEL remain rejected.
pub fn validate_path_segment(segment: &str) -> bool {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
        || segment.chars().any(is_forbidden_path_segment_char)
    {
        return false;
    }

    true
}

fn validate_third_party_segment(segment: &str) -> Result<(), ThirdPartyPathError> {
    if validate_path_segment(segment) {
        Ok(())
    } else {
        Err(ThirdPartyPathError::InvalidPath)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn validates_browser_asset_path_segments() {
        assert!(validate_path_segment("avatar.png"));
        assert!(validate_path_segment("ã\u{80}\u{90}.png"));

        assert!(!validate_path_segment(""));
        assert!(!validate_path_segment("."));
        assert!(!validate_path_segment(".."));
        assert!(!validate_path_segment("a/b.png"));
        assert!(!validate_path_segment("a\\b.png"));
        assert!(!validate_path_segment("bad:name.png"));
        assert!(!validate_path_segment("bad\u{001F}.png"));
        assert!(!validate_path_segment("bad\u{007F}.png"));
    }

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
    fn parses_legacy_c1_background_asset_path() {
        let path = "/backgrounds/%C3%A3%C2%80%C2%90.png";
        let parsed = parse_user_data_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.kind, UserDataAssetKind::Background);
        assert_eq!(parsed.relative_path, PathBuf::from("ã\u{80}\u{90}.png"));
        assert_eq!(parsed.relative_path_display, "ã\u{80}\u{90}.png");
    }

    #[test]
    fn rejects_c0_control_path_segments() {
        let path = "/backgrounds/bad%1F.png";
        let result = parse_user_data_asset_request_path(path);
        assert_eq!(result, Err(UserDataPathError::InvalidPath));
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

    #[test]
    fn parses_valid_third_party_asset_path() {
        let path = "/scripts/extensions/third-party/mobile/manifest.json";
        let parsed = parse_third_party_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.extension_folder, "mobile");
        assert_eq!(parsed.relative_path, PathBuf::from("manifest.json"));
        assert_eq!(parsed.relative_path_display, "manifest.json");
    }

    #[test]
    fn parses_legacy_c1_third_party_asset_path_segments() {
        let path = "/scripts/extensions/third-party/%C3%A3%C2%80%C2%90/%C3%A3%C2%80%C2%90.js";
        let parsed = parse_third_party_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.extension_folder, "ã\u{80}\u{90}");
        assert_eq!(parsed.relative_path, PathBuf::from("ã\u{80}\u{90}.js"));
        assert_eq!(parsed.relative_path_display, "ã\u{80}\u{90}.js");
    }

    #[test]
    fn normalizes_redundant_third_party_relative_separators() {
        let path = "/scripts/extensions/third-party/mobile//a.js";
        let parsed = parse_third_party_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.extension_folder, "mobile");
        assert_eq!(parsed.relative_path, PathBuf::from("a.js"));
        assert_eq!(parsed.relative_path_display, "a.js");
    }

    #[test]
    fn rejects_third_party_dot_segments() {
        let path = "/scripts/extensions/third-party/mobile/../a.js";
        let result = parse_third_party_asset_request_path(path);
        assert_eq!(result, Err(ThirdPartyPathError::InvalidPath));
    }

    #[test]
    fn rejects_third_party_encoded_path_separators() {
        let path = "/scripts/extensions/third-party/mobile/%2fsecret.js";
        let result = parse_third_party_asset_request_path(path);
        assert_eq!(result, Err(ThirdPartyPathError::InvalidPath));
    }

    #[test]
    fn rejects_third_party_c0_control_segments() {
        let path = "/scripts/extensions/third-party/mobile/bad%1F.js";
        let result = parse_third_party_asset_request_path(path);
        assert_eq!(result, Err(ThirdPartyPathError::InvalidPath));
    }
}
