pub const MAX_SANITIZED_FILENAME_BYTES: usize = 255;

pub(crate) fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = 0usize;
    for (index, ch) in value.char_indices() {
        let next = index + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }

    &value[..end]
}

/// Match SillyTavern's `sanitize-filename@1.6.3` default transformation.
pub fn sanitize_filename(name: &str) -> String {
    fn is_illegal_character(ch: char) -> bool {
        matches!(ch, '/' | '?' | '<' | '>' | '\\' | ':' | '*' | '|' | '"')
    }

    fn is_control_code(ch: char) -> bool {
        let value = ch as u32;
        (0x00..=0x1F).contains(&value) || (0x80..=0x9F).contains(&value)
    }

    fn is_reserved_dots_only(value: &str) -> bool {
        !value.is_empty() && value.chars().all(|ch| ch == '.')
    }

    fn is_windows_reserved_name(value: &str) -> bool {
        if value.is_empty() {
            return false;
        }

        let lower = value.to_ascii_lowercase();
        let stem = lower.split('.').next().unwrap_or(lower.as_str());

        matches!(stem, "con" | "prn" | "aux" | "nul")
            || stem
                .strip_prefix("com")
                .is_some_and(|suffix| suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit())
            || stem
                .strip_prefix("lpt")
                .is_some_and(|suffix| suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit())
    }

    let mut sanitized = String::with_capacity(name.len());
    for ch in name.chars() {
        if is_illegal_character(ch) || is_control_code(ch) {
            continue;
        }

        sanitized.push(ch);
    }

    if is_reserved_dots_only(&sanitized) || is_windows_reserved_name(&sanitized) {
        sanitized.clear();
    }

    while sanitized.ends_with('.') || sanitized.ends_with(' ') {
        sanitized.pop();
    }

    truncate_utf8_bytes(&sanitized, MAX_SANITIZED_FILENAME_BYTES).to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize_filename;

    #[test]
    fn sanitize_filename_removes_illegal_characters() {
        assert_eq!(sanitize_filename("a:b*c?.png"), "abc.png");
        assert_eq!(sanitize_filename("中文/测试"), "中文测试");
    }

    #[test]
    fn sanitize_filename_removes_control_codes() {
        assert_eq!(sanitize_filename("a\u{0000}b"), "ab");
        assert_eq!(sanitize_filename("\u{0080}bad\u{009f}"), "bad");
    }

    #[test]
    fn sanitize_filename_removes_reserved_names() {
        assert_eq!(sanitize_filename("."), "");
        assert_eq!(sanitize_filename(".."), "");
        assert_eq!(sanitize_filename("CON"), "");
        assert_eq!(sanitize_filename("com1.txt"), "");
    }

    #[test]
    fn sanitize_filename_keeps_upstream_reserved_name_ordering() {
        assert_eq!(sanitize_filename("CON "), "CON");
        assert_eq!(sanitize_filename("CON.json "), "");
        assert_eq!(sanitize_filename("LPT9."), "");
    }

    #[test]
    fn sanitize_filename_strips_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("name. "), "name");
        assert_eq!(sanitize_filename("name..."), "name");
    }

    #[test]
    fn sanitize_filename_preserves_leading_spaces_like_upstream() {
        assert_eq!(sanitize_filename(" name "), " name");
        assert_eq!(sanitize_filename("/ name"), " name");
        assert_eq!(sanitize_filename("中文/ 测试"), "中文 测试");
    }

    #[test]
    fn sanitize_filename_truncates_by_utf8_bytes() {
        let long_ascii = "a".repeat(300);
        let sanitized = sanitize_filename(&long_ascii);
        assert_eq!(sanitized.len(), 255);

        let long_cjk = "中".repeat(200);
        let sanitized = sanitize_filename(&long_cjk);
        assert_eq!(sanitized, "中".repeat(85));
        assert_eq!(sanitized.as_bytes().len(), 255);
    }
}
