use std::path::PathBuf;

pub(crate) const THIRD_PARTY_EXTENSION_NAME_PREFIX: &str = "third-party/";
pub(crate) const THIRD_PARTY_EXTENSION_ROUTE_PREFIX: &str = "/scripts/extensions/third-party/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThirdPartyAssetRequestPath {
    pub(crate) extension_folder: String,
    pub(crate) relative_path: PathBuf,
    pub(crate) relative_path_display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThirdPartyPathError {
    MissingExtension,
    MissingAssetPath,
    InvalidPath,
}

pub(crate) fn parse_third_party_extension_folder_name(
    value: &str,
) -> Result<String, ThirdPartyPathError> {
    let normalized = value.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    let normalized = normalized
        .strip_prefix(THIRD_PARTY_EXTENSION_NAME_PREFIX)
        .unwrap_or(normalized);

    if normalized.is_empty() {
        return Err(ThirdPartyPathError::MissingExtension);
    }

    let mut segments = normalized.split('/');
    let folder_name = segments
        .next()
        .ok_or(ThirdPartyPathError::MissingExtension)?;
    validate_path_segment(folder_name)?;

    if segments.next().is_some() {
        return Err(ThirdPartyPathError::InvalidPath);
    }

    Ok(folder_name.to_string())
}

pub(crate) fn sanitize_third_party_extension_folder_name(
    value: &str,
) -> Result<String, ThirdPartyPathError> {
    let sanitized = value
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>();

    validate_path_segment(&sanitized)?;
    Ok(sanitized)
}

pub(crate) fn parse_third_party_asset_request_path(
    path: &str,
) -> Result<Option<ThirdPartyAssetRequestPath>, ThirdPartyPathError> {
    let suffix = match path.strip_prefix(THIRD_PARTY_EXTENSION_ROUTE_PREFIX) {
        Some(value) => value,
        None => return Ok(None),
    };

    let mut raw_segments = suffix.split('/');
    let extension_folder = decode_request_segment(
        raw_segments
            .next()
            .ok_or(ThirdPartyPathError::MissingExtension)?,
    )?;
    validate_path_segment(&extension_folder)?;

    let mut relative_segments = Vec::new();
    for raw_segment in raw_segments {
        if raw_segment.is_empty() {
            continue;
        }

        let segment = decode_request_segment(raw_segment)?;
        validate_path_segment(&segment)?;
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

fn decode_request_segment(segment: &str) -> Result<String, ThirdPartyPathError> {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| ThirdPartyPathError::InvalidPath)
}

fn validate_path_segment(segment: &str) -> Result<(), ThirdPartyPathError> {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
        || segment
            .chars()
            .any(|c| matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(ThirdPartyPathError::InvalidPath);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn parses_extension_folder_name_with_optional_prefix() {
        assert_eq!(
            parse_third_party_extension_folder_name("third-party/mobile").expect("folder name"),
            "mobile"
        );
        assert_eq!(
            parse_third_party_extension_folder_name("/third-party/mobile/").expect("folder name"),
            "mobile"
        );
        assert_eq!(
            parse_third_party_extension_folder_name("mobile").expect("folder name"),
            "mobile"
        );
    }

    #[test]
    fn rejects_nested_extension_identifier() {
        let result = parse_third_party_extension_folder_name("third-party/mobile/nested");
        assert_eq!(result, Err(ThirdPartyPathError::InvalidPath));
    }

    #[test]
    fn sanitizes_install_folder_name() {
        assert_eq!(
            sanitize_third_party_extension_folder_name(" mobile:ext? ").expect("sanitized folder"),
            "mobile_ext_"
        );
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
    fn normalizes_redundant_relative_separators() {
        let path = "/scripts/extensions/third-party/mobile//a.js";
        let parsed = parse_third_party_asset_request_path(path)
            .expect("parse")
            .expect("should match");

        assert_eq!(parsed.extension_folder, "mobile");
        assert_eq!(parsed.relative_path, PathBuf::from("a.js"));
        assert_eq!(parsed.relative_path_display, "a.js");
    }

    #[test]
    fn rejects_dot_segments() {
        let path = "/scripts/extensions/third-party/mobile/../a.js";
        let result = parse_third_party_asset_request_path(path);
        assert_eq!(result, Err(ThirdPartyPathError::InvalidPath));
    }

    #[test]
    fn rejects_encoded_path_separators() {
        let path = "/scripts/extensions/third-party/mobile/%2fsecret.js";
        let result = parse_third_party_asset_request_path(path);
        assert_eq!(result, Err(ThirdPartyPathError::InvalidPath));
    }
}
