use url::Url;

use crate::errors::DomainError;

pub fn parse_user_http_endpoint(raw: &str) -> Result<Url, DomainError> {
    let mut url = parse_endpoint_base(raw)?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(DomainError::InvalidData(
            "User-configured endpoint URL must use http or https".to_string(),
        ));
    }
    if url.host_str().is_none() {
        return Err(DomainError::InvalidData(
            "User-configured endpoint URL must include a host".to_string(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(DomainError::InvalidData(
            "User-configured endpoint URL must not include credentials".to_string(),
        ));
    }

    if url.path() != "/" {
        let path = url.path().trim_end_matches('/').to_string();
        url.set_path(&path);
    }

    Ok(url)
}

pub fn append_endpoint_path(base_url: &str, endpoint_path: &str) -> Result<Url, DomainError> {
    let endpoint_path = endpoint_path.trim();
    if endpoint_path.contains('#') {
        return Err(DomainError::InvalidData(
            "Endpoint path must not include a fragment".to_string(),
        ));
    }
    let (endpoint_path, query) = endpoint_path
        .split_once('?')
        .map_or((endpoint_path, None), |(path, query)| (path, Some(query)));

    let mut url = append_url_segments(
        parse_endpoint_base(base_url)?,
        endpoint_path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty()),
    )?;
    url.set_query(query.filter(|query| !query.is_empty()));
    Ok(url)
}

pub fn append_endpoint_segments(
    base_url: &str,
    endpoint_segments: &[&str],
) -> Result<Url, DomainError> {
    append_url_segments(
        parse_endpoint_base(base_url)?,
        endpoint_segments
            .iter()
            .map(|segment| segment.trim())
            .filter(|segment| !segment.is_empty()),
    )
}

fn append_url_segments<'a>(
    mut url: Url,
    endpoint_segments: impl IntoIterator<Item = &'a str>,
) -> Result<Url, DomainError> {
    let mut endpoint_segments = endpoint_segments.into_iter().peekable();
    {
        let mut url_segments = url.path_segments_mut().map_err(|_| {
            DomainError::InvalidData("Endpoint base URL cannot be used as a path base".to_string())
        })?;
        if endpoint_segments.peek().is_some() {
            url_segments.pop_if_empty();
        }
        url_segments.extend(endpoint_segments);
    }
    Ok(url)
}

fn parse_endpoint_base(base_url: &str) -> Result<Url, DomainError> {
    let url = Url::parse(base_url.trim())
        .map_err(|error| DomainError::InvalidData(format!("Invalid endpoint base URL: {error}")))?;

    if url.query().is_some() || url.fragment().is_some() {
        return Err(DomainError::InvalidData(
            "Endpoint base URL must not include query or fragment".to_string(),
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{append_endpoint_path, append_endpoint_segments, parse_user_http_endpoint};

    #[test]
    fn user_http_endpoint_is_canonical_and_unambiguous() {
        assert_eq!(
            parse_user_http_endpoint(" HTTPS://EXAMPLE.COM:443/v1/// ")
                .unwrap()
                .as_str(),
            "https://example.com/v1"
        );

        for endpoint in [
            "file:///etc/passwd",
            "ftp://example.com/v1",
            "https://user:secret@example.com/v1",
            "https://example.com/v1?target=/admin",
            "https://example.com/v1#models",
        ] {
            assert!(parse_user_http_endpoint(endpoint).is_err(), "{endpoint}");
        }
    }

    #[test]
    fn append_path_preserves_base_path() {
        let url = append_endpoint_path("https://example.com/sd-proxy", "/v1/models").unwrap();

        assert_eq!(url.as_str(), "https://example.com/sd-proxy/v1/models");
    }

    #[test]
    fn append_path_normalizes_slashes() {
        let url = append_endpoint_path("https://example.com/sd-proxy/", "v1/models").unwrap();

        assert_eq!(url.as_str(), "https://example.com/sd-proxy/v1/models");
    }

    #[test]
    fn append_segments_encodes_dynamic_segments() {
        let url =
            append_endpoint_segments("https://example.com/comfy", &["status", "a/b"]).unwrap();

        assert_eq!(url.as_str(), "https://example.com/comfy/status/a%2Fb");
    }

    #[test]
    fn rejects_query_or_fragment_on_base_url() {
        assert!(append_endpoint_path("https://example.com/sd?x=1", "v1/models").is_err());
        assert!(append_endpoint_path("https://example.com/sd#frag", "v1/models").is_err());
    }

    #[test]
    fn append_path_preserves_endpoint_query_and_rejects_fragment() {
        let url = append_endpoint_path("https://example.com/v1", "/models?detailed=true").unwrap();

        assert_eq!(url.as_str(), "https://example.com/v1/models?detailed=true");
        assert!(append_endpoint_path("https://example.com/v1", "/models#internal").is_err());
    }
}
