use reqwest::Url;
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::append_endpoint_path;

pub(crate) fn append_google_api_path(
    base_url: &str,
    api_version: &str,
    suffix: &str,
) -> Result<Url, DomainError> {
    let base_url = base_url.trim_end_matches('/');
    if base_url.ends_with("/v1") || base_url.ends_with("/v1beta") {
        append_endpoint_path(base_url, suffix)
    } else {
        append_endpoint_path(base_url, &format!("{api_version}/{suffix}"))
    }
}

#[cfg(test)]
mod tests {
    use super::append_google_api_path;

    #[test]
    fn google_api_path_keeps_explicit_version_or_adds_default() {
        assert_eq!(
            append_google_api_path("https://example.com", "v1beta", "models")
                .unwrap()
                .as_str(),
            "https://example.com/v1beta/models"
        );
        assert_eq!(
            append_google_api_path("https://example.com/v1", "v1beta", "models")
                .unwrap()
                .as_str(),
            "https://example.com/v1/models"
        );
    }
}
