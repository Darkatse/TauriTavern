use std::fmt;

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

/// Canonical origin key shared by endpoint and import grants, without a trailing slash.
pub fn http_origin(url: &Url) -> Result<String, DomainError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(DomainError::InvalidData(format!(
            "Only http and https URLs can be approved, not {}:",
            url.scheme()
        )));
    }
    Ok(url.origin().ascii_serialization())
}

/// A single DNS label; untrusted values cannot escape a built-in provider's host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLabel(String);

impl HostLabel {
    pub fn parse(name: &str, value: &str) -> Result<Self, DomainError> {
        let value = value.trim();
        let is_label = (1..=63).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !value.starts_with('-')
            && !value.ends_with('-');
        if !is_label {
            return Err(DomainError::InvalidData(format!(
                "{name} must be 1 to 63 letters, digits or hyphens, \
                 not starting or ending with a hyphen: {value:?}"
            )));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HostLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
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
    use url::Url;

    use super::{
        HostLabel, append_endpoint_path, append_endpoint_segments, http_origin,
        parse_user_http_endpoint,
    };

    #[test]
    fn http_origin_keeps_only_scheme_host_and_port() {
        for (url, origin) in [
            (
                "https://user:secret@API.Example.com/v1/tts?token=x#part",
                "https://api.example.com",
            ),
            ("https://example.com:443/v1", "https://example.com"),
            ("http://127.0.0.1:11434/v1", "http://127.0.0.1:11434"),
            ("http://[::1]:8080/", "http://[::1]:8080"),
            ("https://bücher.example/", "https://xn--bcher-kva.example"),
        ] {
            assert_eq!(http_origin(&Url::parse(url).unwrap()).unwrap(), origin);
        }

        for url in [
            "file:///etc/passwd",
            "ftp://example.com/",
            "data:text/plain,x",
        ] {
            assert!(http_origin(&Url::parse(url).unwrap()).is_err(), "{url}");
        }
    }

    #[test]
    fn host_labels_cannot_name_another_host() {
        for label in [
            "us-central1",
            "eastus",
            "us-east-1",
            " eu ",
            &"a".repeat(63),
        ] {
            assert_eq!(
                HostLabel::parse("Region", label).unwrap().as_str(),
                label.trim()
            );
        }

        for value in [
            "evil.example/x?",
            "a.b",
            "evil.example#",
            "user@evil",
            "-east",
            "east-",
            "",
            "  ",
            &"a".repeat(64),
        ] {
            assert!(HostLabel::parse("Region", value).is_err(), "{value:?}");
        }
    }

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
