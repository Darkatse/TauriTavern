//! Transport rules every outbound client follows:
//! - no request reuses an idle connection;
//! - a connection whose peer has been silent for about five minutes is dead.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, ClientBuilder, Error};

use crate::dns::Ipv4First;

const KEEPALIVE_IDLE: Duration = Duration::from_secs(15);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
// Windows fixes the probe count at 10, so the interval alone sets the limit there.
const KEEPALIVE_PROBES: u32 = 10;

#[cfg(target_os = "android")]
fn android_tls_config() -> rustls::ClientConfig {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };

    let mut tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    tls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    tls_config
}

macro_rules! with_transport_rules {
    ($builder:expr, $product_user_agent:expr) => {{
        let builder = $builder
            .user_agent($product_user_agent)
            .pool_max_idle_per_host(0)
            .dns_resolver(Arc::new(Ipv4First))
            .tcp_keepalive(KEEPALIVE_IDLE)
            .tcp_keepalive_interval(KEEPALIVE_INTERVAL)
            .tcp_keepalive_retries(KEEPALIVE_PROBES);
        #[cfg(any(target_os = "android", target_os = "linux"))]
        let builder = builder.tcp_user_timeout(KEEPALIVE_INTERVAL * KEEPALIVE_PROBES);
        #[cfg(target_os = "android")]
        let builder = builder.use_preconfigured_tls(android_tls_config());
        builder
    }};
}

pub(crate) fn build_http_client(
    builder: ClientBuilder,
    product_user_agent: &str,
) -> Result<Client, Error> {
    with_transport_rules!(builder, product_user_agent).build()
}

pub(crate) fn configure_blocking_http_client(
    builder: reqwest::blocking::ClientBuilder,
    product_user_agent: &str,
) -> reqwest::blocking::ClientBuilder {
    with_transport_rules!(builder, product_user_agent)
}
