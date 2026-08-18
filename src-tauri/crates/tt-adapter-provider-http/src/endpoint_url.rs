pub(crate) fn append_google_api_path(base_url: &str, api_version: &str, suffix: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');
    if base_url.ends_with("/v1") || base_url.ends_with("/v1beta") {
        format!("{base_url}/{suffix}")
    } else {
        format!("{base_url}/{api_version}/{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::append_google_api_path;

    #[test]
    fn google_api_path_keeps_explicit_version_or_adds_default() {
        assert_eq!(
            append_google_api_path("https://example.com", "v1beta", "models"),
            "https://example.com/v1beta/models"
        );
        assert_eq!(
            append_google_api_path("https://example.com/v1", "v1beta", "models"),
            "https://example.com/v1/models"
        );
    }
}
