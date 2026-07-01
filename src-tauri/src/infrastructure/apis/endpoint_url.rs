use url::Url;

use tt_domain::errors::DomainError;

pub(crate) fn append_endpoint_path(
    base_url: &str,
    endpoint_path: &str,
) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url.trim())
        .map_err(|error| DomainError::InvalidData(format!("Invalid endpoint base URL: {error}")))?;

    if url.query().is_some() || url.fragment().is_some() {
        return Err(DomainError::InvalidData(
            "Endpoint base URL must not include query or fragment".to_string(),
        ));
    }

    let endpoint_segments = endpoint_path
        .trim()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            DomainError::InvalidData("Endpoint base URL cannot be used as a path base".to_string())
        })?;
        if !endpoint_segments.is_empty() {
            segments.pop_if_empty();
        }
        for segment in endpoint_segments {
            segments.push(segment);
        }
    }

    Ok(url)
}

pub(crate) fn append_endpoint_segments(
    base_url: &str,
    endpoint_segments: &[&str],
) -> Result<Url, DomainError> {
    let mut url = Url::parse(base_url.trim())
        .map_err(|error| DomainError::InvalidData(format!("Invalid endpoint base URL: {error}")))?;

    if url.query().is_some() || url.fragment().is_some() {
        return Err(DomainError::InvalidData(
            "Endpoint base URL must not include query or fragment".to_string(),
        ));
    }

    let endpoint_segments = endpoint_segments
        .iter()
        .map(|segment| segment.trim())
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            DomainError::InvalidData("Endpoint base URL cannot be used as a path base".to_string())
        })?;
        if !endpoint_segments.is_empty() {
            segments.pop_if_empty();
        }
        for segment in endpoint_segments {
            segments.push(segment);
        }
    }

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{append_endpoint_path, append_endpoint_segments};

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
}
