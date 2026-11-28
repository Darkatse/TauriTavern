use std::collections::HashSet;
use std::net::IpAddr;

use reqwest::Url;
use reqwest::redirect::Policy;
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::http_origin;

const MAX_USER_ENDPOINT_REDIRECTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UserEndpointRoute {
    Direct,
    Default,
}

pub(crate) fn restricted_redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        if let Some(reason) = redirect_rejection(attempt.previous(), attempt.url()) {
            attempt.error(reason)
        } else {
            attempt.follow()
        }
    })
}

fn redirect_rejection(previous: &[Url], next: &Url) -> Option<&'static str> {
    if previous.len() > MAX_USER_ENDPOINT_REDIRECTS {
        return Some("user endpoint redirect exceeded 5 hops");
    }
    if !next.username().is_empty() || next.password().is_some() {
        return Some("user endpoint redirect must not include credentials");
    }
    if !previous
        .last()
        .is_some_and(|current| current.origin() == next.origin())
    {
        return Some("user endpoint redirect must remain on the same origin");
    }
    None
}

pub(crate) fn user_endpoint_route(
    url: &str,
    grants: &HashSet<String>,
) -> Result<UserEndpointRoute, DomainError> {
    let url = Url::parse(url)
        .map_err(|error| DomainError::InvalidData(format!("Invalid user endpoint URL: {error}")))?;
    let origin = http_origin(&url)?;
    if !grants.contains(&origin) {
        return Err(DomainError::InvalidData(format!(
            "User-configured endpoint requires approval: {origin}"
        )));
    }

    let host = normalize_host(url.host_str().expect("http(s) URLs have a host"));
    let direct = explicit_loopback_host(&host)
        || host.parse::<IpAddr>().is_ok_and(is_explicit_local_address);

    Ok(if direct {
        UserEndpointRoute::Direct
    } else {
        UserEndpointRoute::Default
    })
}

fn normalize_host(host: &str) -> String {
    let host = host.trim_end_matches('.');
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase()
}

fn explicit_loopback_host(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost")
}

fn is_explicit_local_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_loopback() || address.is_private(),
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unique_local()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|address| address.is_loopback() || address.is_private())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_are_bounded_and_same_origin() {
        let current = Url::parse("https://example.com/v1/models").unwrap();
        let same_origin = Url::parse("https://example.com/v1/models?page=2").unwrap();
        let cross_origin = Url::parse("https://other.example/v1/models").unwrap();
        let downgrade = Url::parse("http://example.com/v1/models").unwrap();

        assert_eq!(
            redirect_rejection(std::slice::from_ref(&current), &same_origin),
            None
        );
        assert!(redirect_rejection(std::slice::from_ref(&current), &cross_origin).is_some());
        assert!(redirect_rejection(std::slice::from_ref(&current), &downgrade).is_some());
        assert!(
            redirect_rejection(
                &vec![current.clone(); MAX_USER_ENDPOINT_REDIRECTS + 1],
                &same_origin
            )
            .is_some()
        );
    }

    fn granted(url: &str) -> HashSet<String> {
        HashSet::from([http_origin(&Url::parse(url).unwrap()).unwrap()])
    }

    #[test]
    fn user_endpoint_grants_cover_their_origin() {
        for (url, origin) in [
            ("https://api.example.com/v1", "https://api.example.com"),
            (
                "http://model-server.local:11434/v1",
                "http://model-server.local:11434",
            ),
            ("http://192.168.1.2:11434/v1", "http://192.168.1.2:11434"),
            ("http://198.18.0.5/v1", "http://198.18.0.5"),
            ("http://localhost:11434/v1", "http://localhost:11434"),
        ] {
            assert!(user_endpoint_route(url, &HashSet::new()).is_err());
            assert!(user_endpoint_route(url, &HashSet::from([origin.to_string()])).is_ok());
        }

        let grants = HashSet::from(["https://api.example.com".to_string()]);
        assert!(user_endpoint_route("https://api.example.com/other?page=2", &grants).is_ok());
        for other in [
            "http://api.example.com/v1",
            "https://api.example.com:8443/v1",
            "https://example.com/v1",
            "https://api.example.com.evil.example/v1",
        ] {
            assert!(user_endpoint_route(other, &grants).is_err(), "{other}");
        }
        assert!(user_endpoint_route("file:///etc/passwd", &HashSet::new()).is_err());
    }

    #[test]
    fn only_explicit_local_hosts_force_direct_routing() {
        for endpoint in [
            "http://127.0.0.1:11434/v1",
            "http://10.0.0.1/v1",
            "http://172.16.0.1/v1",
            "http://192.168.0.1/v1",
            "http://[::1]/v1",
            "http://[::ffff:7f00:1]/v1",
            "http://[fc00::1]/v1",
            "http://localhost:11434/v1",
            "http://api.localhost:11434/v1",
        ] {
            assert_eq!(
                user_endpoint_route(endpoint, &granted(endpoint)).unwrap(),
                UserEndpointRoute::Direct,
                "{endpoint}"
            );
        }

        for endpoint in [
            "https://api.example.com/v1",
            "http://model-server.local:11434/v1",
            "http://198.18.0.5/v1",
            "http://169.254.169.254/latest",
        ] {
            assert_eq!(
                user_endpoint_route(endpoint, &granted(endpoint)).unwrap(),
                UserEndpointRoute::Default,
                "{endpoint}"
            );
        }
    }
}
