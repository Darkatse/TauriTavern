pub(crate) fn decode_request_segment(segment: &str) -> Result<String, ()> {
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
pub(crate) fn validate_path_segment(segment: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
