use reqwest::{Client, ClientBuilder, Error};

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

#[cfg(target_os = "android")]
fn apply_android_tls(builder: ClientBuilder) -> ClientBuilder {
    builder.use_preconfigured_tls(android_tls_config())
}

pub(crate) fn build_http_client(
    builder: ClientBuilder,
    product_user_agent: &str,
) -> Result<Client, Error> {
    let builder = builder.user_agent(product_user_agent);
    #[cfg(target_os = "android")]
    let builder = apply_android_tls(builder);
    builder.build()
}

pub(crate) fn configure_blocking_http_client(
    builder: reqwest::blocking::ClientBuilder,
    product_user_agent: &str,
) -> reqwest::blocking::ClientBuilder {
    let builder = builder.user_agent(product_user_agent);
    #[cfg(target_os = "android")]
    let builder = builder.use_preconfigured_tls(android_tls_config());
    builder
}
