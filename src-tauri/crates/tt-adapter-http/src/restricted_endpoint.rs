use std::error::Error;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use reqwest::Url;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::parse_user_http_endpoint;

const MAX_USER_ENDPOINT_REDIRECTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UserEndpointRoute {
    Direct,
    DefaultTransport,
}

#[derive(Debug, Clone)]
pub(crate) struct RestrictedEndpointResolver {
    trusted_proxy_host: Option<String>,
}

impl RestrictedEndpointResolver {
    pub(crate) fn new(trusted_proxy_host: Option<&str>) -> Self {
        Self {
            trusted_proxy_host: trusted_proxy_host.map(normalize_dns_name),
        }
    }
}

impl Resolve for RestrictedEndpointResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let normalized_host = normalize_dns_name(&host);
        let trust_all_addresses = self
            .trusted_proxy_host
            .as_deref()
            .is_some_and(|proxy_host| normalized_host == proxy_host);
        let allow_loopback = explicit_loopback_host(&normalized_host);

        Box::pin(async move {
            let resolved = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)?;
            let addresses =
                filter_resolved_addresses(resolved, trust_all_addresses, allow_loopback);

            if addresses.is_empty() {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("endpoint host `{host}` did not resolve to a permitted address"),
                )) as Box<dyn Error + Send + Sync>);
            }

            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
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

pub(crate) fn user_endpoint_route(base_url: &str) -> Result<UserEndpointRoute, DomainError> {
    let url = parse_user_http_endpoint(base_url)?;
    let host = url
        .host_str()
        .expect("parse_user_http_endpoint guarantees a host");
    let normalized_host = normalize_dns_name(host);
    if explicit_loopback_host(&normalized_host) {
        return Ok(UserEndpointRoute::Direct);
    }

    let Ok(address) = normalized_host.parse::<IpAddr>() else {
        return Ok(UserEndpointRoute::DefaultTransport);
    };
    if !is_globally_reachable_address(address) {
        return Err(DomainError::InvalidData(format!(
            "User-configured endpoint address is not public or loopback: {address}"
        )));
    }

    Ok(UserEndpointRoute::DefaultTransport)
}

fn normalize_dns_name(host: &str) -> String {
    let host = host.trim_end_matches('.');
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase()
}

fn explicit_loopback_host(normalized_host: &str) -> bool {
    normalized_host == "localhost"
        || normalized_host.ends_with(".localhost")
        || normalized_host
            .parse::<IpAddr>()
            .is_ok_and(is_loopback_address)
}

fn filter_resolved_addresses(
    addresses: impl Iterator<Item = SocketAddr>,
    trust_all_addresses: bool,
    allow_loopback: bool,
) -> Vec<SocketAddr> {
    addresses
        .filter(|address| {
            trust_all_addresses
                || is_globally_reachable_address(address.ip())
                || (allow_loopback && is_loopback_address(address.ip()))
        })
        .collect()
}

// Public means globally reachable according to the IANA IPv4/IPv6 special-purpose
// registries (reviewed 2026-08-16). The resolver admits loopback separately only for
// explicit localhost names and literals.
// https://www.iana.org/assignments/iana-ipv4-special-registry/
// https://www.iana.org/assignments/iana-ipv6-special-registry/
fn is_globally_reachable_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_globally_reachable_ipv4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_globally_reachable_ipv4(mapped);
            }
            if ipv6_in_prefix(address, Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 96) {
                let octets = address.octets();
                let embedded = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
                return is_globally_reachable_ipv4(embedded);
            }
            is_globally_reachable_ipv6(address)
        }
    }
}

fn is_loopback_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_loopback(),
        IpAddr::V6(address) => {
            address.is_loopback() || address.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
        }
    }
}

fn is_globally_reachable_ipv4(address: Ipv4Addr) -> bool {
    if matches!(address.octets(), [192, 0, 0, 9 | 10]) {
        return true;
    }

    const NON_GLOBAL: &[([u8; 4], u8)] = &[
        ([0, 0, 0, 0], 8),
        ([10, 0, 0, 0], 8),
        ([100, 64, 0, 0], 10),
        ([127, 0, 0, 0], 8),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 0, 0, 0], 24),
        ([192, 0, 2, 0], 24),
        ([192, 88, 99, 0], 24),
        ([192, 168, 0, 0], 16),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([224, 0, 0, 0], 4),
        ([240, 0, 0, 0], 4),
    ];

    !NON_GLOBAL
        .iter()
        .any(|(network, prefix)| ipv4_in_prefix(address, *network, *prefix))
}

fn is_globally_reachable_ipv6(address: Ipv6Addr) -> bool {
    const IETF_PROTOCOL_ASSIGNMENTS: (Ipv6Addr, u8) =
        (Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23);
    const GLOBAL_EXCEPTIONS: &[(Ipv6Addr, u8)] = &[
        (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 1), 128),
        (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 2), 128),
        (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 3), 128),
        (Ipv6Addr::new(0x2001, 3, 0, 0, 0, 0, 0, 0), 32),
        (Ipv6Addr::new(0x2001, 4, 0x112, 0, 0, 0, 0, 0), 48),
        (Ipv6Addr::new(0x2001, 0x20, 0, 0, 0, 0, 0, 0), 28),
        (Ipv6Addr::new(0x2001, 0x30, 0, 0, 0, 0, 0, 0), 28),
    ];

    if ipv6_in_prefix(
        address,
        IETF_PROTOCOL_ASSIGNMENTS.0,
        IETF_PROTOCOL_ASSIGNMENTS.1,
    ) {
        return GLOBAL_EXCEPTIONS
            .iter()
            .any(|(network, prefix)| ipv6_in_prefix(address, *network, *prefix));
    }

    const NON_GLOBAL: &[(Ipv6Addr, u8)] = &[
        (Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0), 32),
        (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16),
        (Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20),
    ];
    ipv6_in_prefix(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3)
        && !NON_GLOBAL
            .iter()
            .any(|(network, prefix)| ipv6_in_prefix(address, *network, *prefix))
}

fn ipv4_in_prefix(address: Ipv4Addr, network: [u8; 4], prefix: u8) -> bool {
    let shift = 32 - u32::from(prefix);
    u32::from_be_bytes(address.octets()) >> shift == u32::from_be_bytes(network) >> shift
}

fn ipv6_in_prefix(address: Ipv6Addr, network: Ipv6Addr, prefix: u8) -> bool {
    let shift = 128 - u32::from(prefix);
    u128::from_be_bytes(address.octets()) >> shift == u128::from_be_bytes(network.octets()) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_are_bounded_and_same_origin() {
        let current = Url::parse("https://api.example.com/v1").unwrap();
        let same_origin = Url::parse("https://api.example.com/models?page=2").unwrap();
        let local_current = Url::parse("http://localhost:11434/v1").unwrap();
        let local_next = Url::parse("http://localhost:11434/v1/").unwrap();

        assert_eq!(
            redirect_rejection(std::slice::from_ref(&current), &same_origin),
            None
        );
        assert_eq!(redirect_rejection(&[local_current], &local_next), None);
        for target in [
            "https://other.example.com/v1",
            "http://api.example.com/v1",
            "https://user:secret@api.example.com/v1",
        ] {
            assert!(
                redirect_rejection(std::slice::from_ref(&current), &Url::parse(target).unwrap())
                    .is_some(),
                "{target}"
            );
        }
        assert_eq!(
            redirect_rejection(
                &vec![current.clone(); MAX_USER_ENDPOINT_REDIRECTS],
                &same_origin
            ),
            None
        );
        assert!(
            redirect_rejection(
                &vec![current.clone(); MAX_USER_ENDPOINT_REDIRECTS + 1],
                &same_origin
            )
            .is_some()
        );
    }

    #[test]
    fn address_policy_identifies_globally_reachable_addresses() {
        for address in [
            "8.8.8.8",
            "192.0.0.9",
            "2606:4700:4700::1111",
            "64:ff9b::808:808",
        ] {
            assert!(
                is_globally_reachable_address(address.parse().unwrap()),
                "{address}"
            );
        }

        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.100.100.200",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "240.0.0.1",
            "127.0.0.1",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a00:1",
            "2001:db8::1",
            "3fff::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
        ] {
            assert!(
                !is_globally_reachable_address(address.parse().unwrap()),
                "{address}"
            );
        }
    }

    #[test]
    fn endpoint_route_rejects_unsafe_literals_and_forces_loopback_direct() {
        assert_eq!(
            user_endpoint_route("http://localhost:8080/v1").unwrap(),
            UserEndpointRoute::Direct
        );
        assert_eq!(
            user_endpoint_route("http://[::ffff:127.0.0.1]:8080/v1").unwrap(),
            UserEndpointRoute::Direct
        );
        assert_eq!(
            user_endpoint_route("http://2130706433:8080/v1").unwrap(),
            UserEndpointRoute::Direct
        );
        assert_eq!(
            user_endpoint_route("https://api.example.com/v1").unwrap(),
            UserEndpointRoute::DefaultTransport
        );
        assert!(user_endpoint_route("http://192.168.1.2/v1").is_err());
        assert!(user_endpoint_route("http://0xc0a80102/v1").is_err());
        assert!(user_endpoint_route("http://[fc00::1]/v1").is_err());
        assert!(user_endpoint_route("http://[fe80::1%25en0]/v1").is_err());
        assert!(user_endpoint_route("file:///etc/passwd").is_err());
    }

    #[test]
    fn mixed_dns_results_keep_only_permitted_addresses() {
        let addresses = [
            "127.0.0.1:443".parse().unwrap(),
            "10.0.0.1:443".parse().unwrap(),
            "93.184.216.34:443".parse().unwrap(),
            "[fc00::1]:443".parse().unwrap(),
            "[2606:2800:220:1:248:1893:25c8:1946]:443".parse().unwrap(),
        ];

        let public_only = filter_resolved_addresses(addresses.into_iter(), false, false);
        let with_loopback = filter_resolved_addresses(addresses.into_iter(), false, true);

        assert_eq!(public_only, [addresses[2], addresses[4]]);
        assert_eq!(with_loopback, [addresses[0], addresses[2], addresses[4]]);
    }
}
