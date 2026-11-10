use std::net::SocketAddr;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// System resolution with IPv4 addresses first; IPv6 addresses stay as the fallback.
pub(crate) struct Ipv4First;

impl Resolve for Ipv4First {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let mut addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            addrs.sort_by_key(SocketAddr::is_ipv6);
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}
