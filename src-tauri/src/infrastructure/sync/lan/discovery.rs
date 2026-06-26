use async_trait::async_trait;
use local_ip_address::{list_afinet_netifas, local_ip};
use url::Url;

use crate::application::services::lan_sync_service::LanAddressDiscovery;
use crate::domain::errors::DomainError;

pub struct LocalLanAddressDiscovery;

#[async_trait]
impl LanAddressDiscovery for LocalLanAddressDiscovery {
    fn list_available_addresses(&self, port: u16) -> Result<Vec<String>, DomainError> {
        let ifas =
            list_afinet_netifas().map_err(|error| DomainError::InternalError(error.to_string()))?;

        let mut addresses = ifas
            .into_iter()
            .filter_map(|(_name, ip)| match ip {
                std::net::IpAddr::V4(ip) => {
                    if ip.is_loopback() || ip.is_unspecified() {
                        None
                    } else {
                        Some(format!("https://{}:{}", ip, port))
                    }
                }
                std::net::IpAddr::V6(_) => None,
            })
            .collect::<Vec<_>>();

        addresses.sort();
        addresses.dedup();
        Ok(addresses)
    }

    fn default_advertise_address(
        &self,
        port: u16,
        available_addresses: &[String],
    ) -> Option<String> {
        let route_ip = local_ip().ok().and_then(|ip| match ip {
            std::net::IpAddr::V4(v4) => Some(format!("https://{}:{}", v4, port)),
            std::net::IpAddr::V6(_) => None,
        });

        route_ip
            .filter(|addr| available_addresses.contains(addr))
            .or_else(|| available_addresses.first().cloned())
    }

    async fn routed_advertise_address(
        &self,
        peer_base_url: &str,
        local_port: u16,
    ) -> Result<String, DomainError> {
        let peer_url = Url::parse(peer_base_url)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        if peer_url.scheme() != "https" {
            return Err(DomainError::InvalidData(
                "LAN Sync peer URL must use https".to_string(),
            ));
        }
        let peer_host = peer_url.host_str().ok_or_else(|| {
            DomainError::InvalidData("LAN Sync peer URL is missing host".to_string())
        })?;
        let peer_port = peer_url.port_or_known_default().ok_or_else(|| {
            DomainError::InvalidData("LAN Sync peer URL is missing port".to_string())
        })?;

        let remote_addr = tokio::net::lookup_host((peer_host, peer_port))
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?
            .find(|addr| addr.is_ipv4())
            .ok_or_else(|| {
                DomainError::InvalidData("No IPv4 LAN Sync peer address resolved".to_string())
            })?;

        let socket = tokio::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        socket
            .connect(remote_addr)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        let local_addr = socket
            .local_addr()
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        match local_addr.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_unspecified() => {
                Ok(format!("https://{}:{}", ip, local_port))
            }
            _ => Err(DomainError::InvalidData(
                "No routable IPv4 LAN Sync address".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn routed_lan_advertise_address_uses_peer_route() {
        let address = LocalLanAddressDiscovery
            .routed_advertise_address("https://127.0.0.1:50000", 56000)
            .await
            .expect("routed address");

        assert_eq!(address, "https://127.0.0.1:56000");
    }
}
