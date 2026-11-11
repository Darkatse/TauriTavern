use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

use if_addrs::IfAddr;
use url::Url;

use tt_domain::errors::DomainError;
use tt_ports::lan_sync::{LanAddressDiscovery, LocalLanAddresses};

/// TEST-NET-1 (RFC 5737) is never on-link, so routing to it selects the default route.
const DEFAULT_ROUTE_PROBE: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 9));

pub struct LocalLanAddressDiscovery;

impl LanAddressDiscovery for LocalLanAddressDiscovery {
    fn local_addresses(&self, port: u16) -> Result<LocalLanAddresses, DomainError> {
        let interfaces = if_addrs::get_if_addrs().map_err(|error| {
            DomainError::InternalError(format!("Failed to enumerate LAN addresses: {error}"))
        })?;
        // Address -> whether it is on a running broadcast interface.
        let mut addresses = BTreeMap::<Ipv4Addr, bool>::new();
        for interface in &interfaces {
            let IfAddr::V4(address) = &interface.addr else {
                continue;
            };
            if address.ip.is_loopback() || address.ip.is_unspecified() {
                continue;
            }
            *addresses.entry(address.ip).or_default() |=
                interface.is_oper_up() && address.broadcast.is_some();
        }

        // The probe is only a preference; when it fails, candidate order decides.
        let route = route_source(DEFAULT_ROUTE_PROBE)
            .inspect_err(|error| tracing::debug!(%error, "LAN default route probe failed"))
            .ok();
        Ok(LocalLanAddresses {
            default: select_default(&addresses, route).map(|ip| lan_url(ip, port)),
            available: addresses.keys().map(|&ip| lan_url(ip, port)).collect(),
        })
    }
}

/// Candidates are the broadcast-interface addresses, or all addresses when there are none.
/// The route wins among candidates; otherwise the lowest address does.
fn select_default(
    addresses: &BTreeMap<Ipv4Addr, bool>,
    route: Option<Ipv4Addr>,
) -> Option<Ipv4Addr> {
    let broadcast_only = addresses.values().any(|&broadcast| broadcast);
    let is_candidate = |ip: &Ipv4Addr| {
        addresses
            .get(ip)
            .is_some_and(|&broadcast| broadcast || !broadcast_only)
    };
    route
        .filter(is_candidate)
        .or_else(|| addresses.keys().copied().find(is_candidate))
}

fn lan_url(ip: Ipv4Addr, port: u16) -> String {
    format!("https://{ip}:{port}")
}

/// A UDP connect sends nothing; the kernel only resolves the route and its source address.
fn route_source(remote: SocketAddr) -> io::Result<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect(remote)?;
    match socket.local_addr()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Ok(ip),
        _ => Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "No routable IPv4 LAN Sync address",
        )),
    }
}

pub(crate) async fn routed_advertise_address(
    peer_base_url: &str,
    local_port: u16,
) -> Result<String, DomainError> {
    let peer_url =
        Url::parse(peer_base_url).map_err(|error| DomainError::InvalidData(error.to_string()))?;
    if peer_url.scheme() != "https" {
        return Err(DomainError::InvalidData(
            "LAN Sync peer URL must use https".to_string(),
        ));
    }
    let peer_host = peer_url
        .host_str()
        .ok_or_else(|| DomainError::InvalidData("LAN Sync peer URL is missing host".to_string()))?;
    let peer_port = peer_url
        .port_or_known_default()
        .ok_or_else(|| DomainError::InvalidData("LAN Sync peer URL is missing port".to_string()))?;
    let remote_addr = tokio::net::lookup_host((peer_host, peer_port))
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .find(|addr| addr.is_ipv4())
        .ok_or_else(|| {
            DomainError::InvalidData("No IPv4 LAN Sync peer address resolved".to_string())
        })?;
    let ip = route_source(remote_addr).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to determine LAN Sync return address: {error}"
        ))
    })?;
    Ok(lan_url(ip, local_port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_address_prefers_broadcast_interfaces_then_the_route() {
        let wifi = Ipv4Addr::new(192, 168, 1, 20);
        let ethernet = Ipv4Addr::new(192, 168, 1, 10);
        let tunnel = Ipv4Addr::new(100, 64, 0, 1);
        for (interfaces, route, expected) in [
            (
                vec![(wifi, true), (ethernet, true), (tunnel, false)],
                Some(wifi),
                Some(wifi),
            ),
            (
                vec![(wifi, true), (tunnel, false)],
                Some(tunnel),
                Some(wifi),
            ),
            (vec![(wifi, false), (tunnel, false)], Some(wifi), Some(wifi)),
            (vec![(wifi, true), (ethernet, true)], None, Some(ethernet)),
            (vec![], None, None),
        ] {
            assert_eq!(
                select_default(&interfaces.into_iter().collect(), route),
                expected
            );
        }
    }

    #[tokio::test]
    async fn routed_lan_advertise_address_uses_peer_route() {
        let address = routed_advertise_address("https://127.0.0.1:50000", 56000)
            .await
            .expect("routed address");
        assert_eq!(address, "https://127.0.0.1:56000");
    }
}
