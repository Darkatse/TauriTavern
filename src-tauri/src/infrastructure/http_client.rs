use reqwest::{Client, ClientBuilder, Error};

/// Keep a stable product token so upstream API gateways can whitelist requests.
pub const APP_USER_AGENT: &str = concat!("TauriTavern/", env!("CARGO_PKG_VERSION"));

pub fn apply_default_user_agent(builder: ClientBuilder) -> ClientBuilder {
    builder.user_agent(APP_USER_AGENT)
}

#[cfg(target_os = "android")]
fn apply_android_tls(builder: ClientBuilder) -> ClientBuilder {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };

    let mut tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    tls_config.alpn_protocols = vec![b"http/1.1".to_vec()];

    builder.use_preconfigured_tls(tls_config)
}

pub fn build_http_client(builder: ClientBuilder) -> Result<Client, Error> {
    let builder = apply_default_user_agent(builder);
    #[cfg(target_os = "android")]
    let builder = apply_android_tls(builder);
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::APP_USER_AGENT;

    #[test]
    fn app_user_agent_matches_package_version() {
        assert_eq!(
            APP_USER_AGENT,
            concat!("TauriTavern/", env!("CARGO_PKG_VERSION"))
        );
    }
}
