use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::blocking::{Client as BlockingClient, ClientBuilder as BlockingClientBuilder};
use reqwest::redirect::Policy;
use reqwest::{Client, NoProxy, Proxy, Url};
use tt_domain::errors::DomainError;
use tt_domain::models::settings::RequestProxySettings;
use tt_ports::local_endpoint_access::{LocalEndpointAccessRuntime, LocalEndpointCandidate};
use tt_ports::settings::RequestProxyRuntime;

use crate::client::{build_http_client, configure_blocking_http_client};
use crate::restricted_endpoint::{
    RestrictedEndpointResolver, UserEndpointRoute, inspect_user_endpoint,
    restricted_redirect_policy, user_endpoint_route,
};

pub const CHAT_COMPLETION_CONNECT_TIMEOUT: Duration = Duration::from_secs(3 * 60);
pub const CHAT_COMPLETION_NON_STREAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const TOKENIZER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const TOKENIZER_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const PROVIDER_METADATA_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const PROVIDER_METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const IMAGE_GENERATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const TRANSLATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const TRANSLATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const TTS_CONNECT_TIMEOUT: Duration = Duration::from_secs(3 * 60);
pub const TTS_REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const GIT_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
pub const GIT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const MCP_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpClientProfile {
    Default,
    Download,
    Tokenizer,
    ChatCompletion,
    ChatCompletionStream,
    ChatCompletionWebSocket,
    ProviderMetadata,
    ImageGeneration,
    Translation,
    Tts,
    Mcp,
}

#[derive(Clone)]
struct ConfiguredProxy {
    proxy: Proxy,
    host: String,
}

#[derive(Clone, Default)]
enum RequestProxyState {
    #[default]
    Disabled,
    Configured(Box<ConfiguredProxy>),
    Invalid,
}

impl RequestProxyState {
    fn configured(&self) -> Result<Option<ConfiguredProxy>, DomainError> {
        match self {
            Self::Disabled => Ok(None),
            Self::Configured(proxy) => Ok(Some(proxy.as_ref().clone())),
            Self::Invalid => Err(DomainError::InvalidData(
                "Request proxy settings are invalid; update or disable the proxy in TauriTavern Settings"
                    .to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ClientCacheKey {
    profile: HttpClientProfile,
    user_endpoint_route: Option<UserEndpointRoute>,
}

#[derive(Default)]
struct HttpClientPoolState {
    revision: u64,
    proxy: RequestProxyState,
    local_endpoint_grants: HashSet<String>,
    clients: HashMap<ClientCacheKey, Client>,
}

pub struct HttpClientPool {
    product_user_agent: String,
    state: RwLock<HttpClientPoolState>,
}

impl HttpClientPool {
    pub fn new(product_user_agent: impl Into<String>) -> Self {
        install_rustls_crypto_provider();
        let product_user_agent = product_user_agent.into();
        assert!(
            !product_user_agent.trim().is_empty(),
            "HTTP product user-agent must not be empty"
        );

        Self {
            product_user_agent,
            state: RwLock::new(HttpClientPoolState::default()),
        }
    }

    pub fn validate_request_proxy_settings(
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError> {
        let _ = proxy_from_settings(settings)?;
        Ok(())
    }

    pub fn apply_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError> {
        let proxy = proxy_from_settings(settings)?;

        self.replace_request_proxy(
            proxy
                .map(Box::new)
                .map_or(RequestProxyState::Disabled, RequestProxyState::Configured),
        );
        Ok(())
    }

    /// Loads persisted settings without ever degrading an invalid proxy to direct transport.
    pub fn apply_persisted_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError> {
        match proxy_from_settings(settings) {
            Ok(proxy) => self.replace_request_proxy(
                proxy
                    .map(Box::new)
                    .map_or(RequestProxyState::Disabled, RequestProxyState::Configured),
            ),
            Err(error) => {
                self.replace_request_proxy(RequestProxyState::Invalid);
                return Err(error);
            }
        }
        Ok(())
    }

    /// Keeps the application repairable while preventing policy-invalid proxy bypass.
    pub fn block_requests_for_invalid_proxy(&self) {
        self.replace_request_proxy(RequestProxyState::Invalid);
    }

    fn replace_request_proxy(&self, proxy: RequestProxyState) {
        let mut state = self.state.write().unwrap();
        state.proxy = proxy;
        state.clients.clear();
        state.revision += 1;
    }

    pub fn client(&self, profile: HttpClientProfile) -> Result<Client, DomainError> {
        self.client_with_revision(profile)
            .map(|(client, _revision)| client)
    }

    pub fn client_with_revision(
        &self,
        profile: HttpClientProfile,
    ) -> Result<(Client, u64), DomainError> {
        self.client_with_revision_for_route(profile, None)
    }

    pub fn user_endpoint_client(
        &self,
        profile: HttpClientProfile,
        base_url: &str,
    ) -> Result<Client, DomainError> {
        self.user_endpoint_client_with_revision(profile, base_url)
            .map(|(client, _revision)| client)
    }

    pub fn user_endpoint_client_with_revision(
        &self,
        profile: HttpClientProfile,
        base_url: &str,
    ) -> Result<(Client, u64), DomainError> {
        let route = {
            let state = self.state.read().unwrap();
            user_endpoint_route(base_url, &state.local_endpoint_grants)?
        };
        self.client_with_revision_for_route(profile, Some(route))
    }

    fn client_with_revision_for_route(
        &self,
        profile: HttpClientProfile,
        user_endpoint_route: Option<UserEndpointRoute>,
    ) -> Result<(Client, u64), DomainError> {
        let key = ClientCacheKey {
            profile,
            user_endpoint_route,
        };
        loop {
            let (revision, proxy) = {
                let state = self.state.read().unwrap();
                if let Some(client) = state.clients.get(&key) {
                    return Ok((client.clone(), state.revision));
                }

                let proxy = if user_endpoint_route == Some(UserEndpointRoute::TrustedDirect) {
                    None
                } else {
                    state.proxy.configured()?
                };
                (state.revision, proxy)
            };

            let client = build_profile_client(
                profile,
                proxy,
                user_endpoint_route,
                &self.product_user_agent,
            )?;

            let mut state = self.state.write().unwrap();
            if state.revision != revision {
                continue;
            }

            match state.clients.entry(key) {
                Entry::Occupied(entry) => return Ok((entry.get().clone(), state.revision)),
                Entry::Vacant(entry) => {
                    entry.insert(client.clone());
                    return Ok((client, state.revision));
                }
            }
        }
    }

    pub fn git_blocking_client_builder(&self) -> Result<BlockingClientBuilder, DomainError> {
        let proxy = self.state.read().unwrap().proxy.configured()?;
        let mut builder = BlockingClient::builder()
            .no_proxy()
            .connect_timeout(GIT_CONNECT_TIMEOUT)
            .timeout(GIT_REQUEST_TIMEOUT);

        if let Some(proxy) = proxy {
            builder = builder.proxy(proxy.proxy);
        }

        Ok(configure_blocking_http_client(
            builder,
            &self.product_user_agent,
        ))
    }
}

fn install_rustls_crypto_provider() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        // Workspace dependencies may compile both rustls providers. Choosing the provider already
        // used by reqwest keeps every TLS consumer deterministic instead of relying on features.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

impl RequestProxyRuntime for HttpClientPool {
    fn validate_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError> {
        Self::validate_request_proxy_settings(settings)
    }

    fn apply_request_proxy_settings(
        &self,
        settings: &RequestProxySettings,
    ) -> Result<(), DomainError> {
        HttpClientPool::apply_request_proxy_settings(self, settings)
    }
}

#[async_trait]
impl LocalEndpointAccessRuntime for HttpClientPool {
    async fn inspect_user_endpoint(
        &self,
        endpoint: &str,
    ) -> Result<Option<LocalEndpointCandidate>, DomainError> {
        inspect_user_endpoint(endpoint).await
    }

    fn replace_local_endpoint_grants(&self, endpoints: &[String]) {
        let grants = endpoints.iter().cloned().collect();
        let mut state = self.state.write().unwrap();
        if state.local_endpoint_grants == grants {
            return;
        }
        state.local_endpoint_grants = grants;
        state.clients.clear();
        state.revision += 1;
    }
}

fn proxy_from_settings(
    settings: &RequestProxySettings,
) -> Result<Option<ConfiguredProxy>, DomainError> {
    if !settings.enabled {
        return Ok(None);
    }

    let url = settings.url.trim();
    if url.is_empty() {
        return Err(DomainError::InvalidData(
            "Request proxy URL is required".to_string(),
        ));
    }

    let proxy_url = Url::parse(url)
        .ok()
        .filter(Url::has_host)
        .map_or_else(|| Url::parse(&format!("http://{url}")), Ok)
        .map_err(|error| DomainError::InvalidData(format!("Invalid request proxy URL: {error}")))?;
    if !matches!(
        proxy_url.scheme(),
        "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
    ) {
        return Err(DomainError::InvalidData(
            "Request proxy URL must use http, https, socks4, socks4a, socks5, or socks5h"
                .to_string(),
        ));
    }
    if !matches!(proxy_url.path(), "" | "/")
        || proxy_url.query().is_some()
        || proxy_url.fragment().is_some()
    {
        return Err(DomainError::InvalidData(
            "Request proxy URL must not include a path, query, or fragment".to_string(),
        ));
    }
    let host = proxy_url
        .host_str()
        .expect("normalized proxy URL guarantees a host")
        .to_string();
    let mut proxy = Proxy::all(proxy_url)
        .map_err(|error| DomainError::InvalidData(format!("Invalid request proxy URL: {error}")))?;

    let bypass = normalized_bypass_csv(&settings.bypass);
    if !bypass.is_empty() {
        proxy = proxy.no_proxy(NoProxy::from_string(&bypass));
    }

    Ok(Some(ConfiguredProxy { proxy, host }))
}

fn normalized_bypass_csv(entries: &[String]) -> String {
    entries
        .iter()
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

fn build_profile_client(
    profile: HttpClientProfile,
    proxy: Option<ConfiguredProxy>,
    user_endpoint_route: Option<UserEndpointRoute>,
    product_user_agent: &str,
) -> Result<Client, DomainError> {
    let mut builder = Client::builder().no_proxy();

    builder = match profile {
        HttpClientProfile::Default => builder,
        HttpClientProfile::Download => builder.redirect(Policy::limited(5)),
        HttpClientProfile::Tokenizer => builder
            .connect_timeout(TOKENIZER_CONNECT_TIMEOUT)
            .timeout(TOKENIZER_REQUEST_TIMEOUT),
        HttpClientProfile::ChatCompletion => builder
            .connect_timeout(CHAT_COMPLETION_CONNECT_TIMEOUT)
            .timeout(CHAT_COMPLETION_NON_STREAM_REQUEST_TIMEOUT),
        HttpClientProfile::ChatCompletionStream => {
            builder.connect_timeout(CHAT_COMPLETION_CONNECT_TIMEOUT)
        }
        HttpClientProfile::ChatCompletionWebSocket => builder
            .http1_only()
            .connect_timeout(CHAT_COMPLETION_CONNECT_TIMEOUT),
        HttpClientProfile::ProviderMetadata => builder
            .connect_timeout(PROVIDER_METADATA_CONNECT_TIMEOUT)
            .timeout(PROVIDER_METADATA_REQUEST_TIMEOUT),
        HttpClientProfile::ImageGeneration => {
            builder.connect_timeout(IMAGE_GENERATION_CONNECT_TIMEOUT)
        }
        HttpClientProfile::Translation => builder
            .connect_timeout(TRANSLATION_CONNECT_TIMEOUT)
            .timeout(TRANSLATION_REQUEST_TIMEOUT),
        HttpClientProfile::Tts => builder
            .connect_timeout(TTS_CONNECT_TIMEOUT)
            .timeout(TTS_REQUEST_TIMEOUT),
        // Match RMCP's default client: idle reuse can stall on delayed ACK when a prior body
        // was not fully consumed.
        HttpClientProfile::Mcp => builder
            .redirect(Policy::none())
            .pool_max_idle_per_host(0)
            .connect_timeout(MCP_CONNECT_TIMEOUT),
    };

    if let Some(route) = user_endpoint_route {
        builder = builder.redirect(restricted_redirect_policy()).dns_resolver(
            RestrictedEndpointResolver::new(proxy.as_ref().map(|proxy| proxy.host.as_str()), route),
        );
    }

    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy.proxy);
    }

    build_http_client(builder, product_user_agent).map_err(|error| {
        DomainError::InternalError(format!("Failed to build HTTP client: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    use super::{HttpClientPool, HttpClientProfile};
    use rcgen::{CertifiedKey, generate_simple_self_signed};
    use reqwest::StatusCode;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{ServerConfig, ServerConnection, StreamOwned};
    use tt_domain::models::settings::RequestProxySettings;
    use tt_ports::local_endpoint_access::LocalEndpointAccessRuntime;

    const TEST_USER_AGENT: &str = "TauriTavern/test";

    fn pool() -> HttpClientPool {
        HttpClientPool::new(TEST_USER_AGENT)
    }

    fn grant_user_endpoint(pool: &HttpClientPool, endpoint: &str) {
        let endpoint = tt_domain::models::endpoint_url::parse_user_http_endpoint(endpoint)
            .unwrap()
            .to_string();
        pool.replace_local_endpoint_grants(&[endpoint]);
    }

    struct CaptureServer {
        url: String,
        requests: Receiver<String>,
        handle: JoinHandle<()>,
    }

    impl CaptureServer {
        fn finish(self) {
            self.handle.join().expect("capture server thread");
        }
    }

    fn capture_server() -> CaptureServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
        let url = format!("http://{}", listener.local_addr().expect("capture address"));
        let (request_tx, requests) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _peer) = listener.accept().expect("accept request");
            let request = read_request_head(&stream).expect("read request");
            request_tx.send(request).expect("send captured request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("write response");
        });
        CaptureServer {
            url,
            requests,
            handle,
        }
    }

    fn proxy_probe(window: Duration) -> (String, Receiver<bool>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind proxy probe");
        listener
            .set_nonblocking(true)
            .expect("nonblocking proxy probe");
        let url = format!("http://{}", listener.local_addr().expect("proxy address"));
        let (hit_tx, hit_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + window;
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _peer)) => {
                        let _ = read_request_head(&stream);
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        hit_tx.send(true).expect("report proxy hit");
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("proxy probe failed: {error}"),
                }
            }
            hit_tx.send(false).expect("report proxy bypass");
        });
        (url, hit_rx, handle)
    }

    fn read_request_head(stream: &TcpStream) -> std::io::Result<String> {
        read_request_head_from(stream.try_clone()?)
    }

    fn read_request_head_from(reader: impl Read) -> std::io::Result<String> {
        let mut reader = BufReader::new(reader);
        let mut request = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            request.push_str(&line);
            if line == "\r\n" || line.is_empty() {
                return Ok(request);
            }
        }
    }

    fn test_tls_config() -> (Arc<ServerConfig>, reqwest::Certificate) {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(["127.0.0.1".to_string()]).expect("test certificate");
        let certificate = cert.der().clone();
        let private_key =
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.clone()], private_key)
            .expect("TLS server config");
        let root = reqwest::Certificate::from_der(certificate.as_ref()).expect("test root");
        (Arc::new(config), root)
    }

    fn tls_server(config: Arc<ServerConfig>) -> (String, Receiver<bool>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS server");
        let url = format!("https://{}", listener.local_addr().expect("TLS address"));
        let (request_tx, request_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (stream, _peer) = listener.accept().expect("accept TLS request");
            let connection = ServerConnection::new(config).expect("TLS connection");
            let mut stream = StreamOwned::new(connection, stream);
            match read_request_head_from(&mut stream) {
                Ok(_request) => {
                    request_tx.send(true).expect("report trusted request");
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .expect("write TLS response");
                }
                Err(_error) => request_tx.send(false).expect("report rejected TLS request"),
            }
        });
        (url, request_rx, handle)
    }

    #[test]
    fn stores_product_user_agent() {
        assert_eq!(pool().product_user_agent, TEST_USER_AGENT);
    }

    #[test]
    fn disabled_proxy_is_valid() {
        let settings = RequestProxySettings {
            enabled: false,
            url: "http://example.com".to_string(),
            bypass: vec![],
        };

        HttpClientPool::validate_request_proxy_settings(&settings).unwrap();
    }

    #[test]
    fn enabled_proxy_requires_url() {
        let settings = RequestProxySettings {
            enabled: true,
            url: "   ".to_string(),
            bypass: vec![],
        };

        let error = HttpClientPool::validate_request_proxy_settings(&settings).unwrap_err();
        assert!(error.to_string().contains("Request proxy URL is required"));
    }

    #[test]
    fn schemeless_proxy_url_is_accepted_and_keeps_its_host() {
        let settings = RequestProxySettings {
            enabled: true,
            url: "proxy.internal:7890".to_string(),
            bypass: vec!["localhost".to_string()],
        };

        let configured = super::proxy_from_settings(&settings).unwrap().unwrap();
        assert_eq!(configured.host, "proxy.internal");
    }

    #[test]
    fn socks_proxy_url_is_accepted() {
        let settings = RequestProxySettings {
            enabled: true,
            url: "socks5://127.0.0.1:1080".to_string(),
            bypass: vec![],
        };

        HttpClientPool::validate_request_proxy_settings(&settings).unwrap();
    }

    #[test]
    fn unsupported_or_ambiguous_proxy_urls_are_rejected() {
        for url in [
            "ftp://proxy.internal:21",
            "http://proxy.internal:7890/path",
            "http://proxy.internal:7890?mode=tunnel",
            "http://proxy.internal:7890#fragment",
        ] {
            let settings = RequestProxySettings {
                enabled: true,
                url: url.to_string(),
                bypass: vec![],
            };

            assert!(
                HttpClientPool::validate_request_proxy_settings(&settings).is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn invalid_startup_proxy_blocks_outbound_clients_until_repaired() {
        let pool = pool();
        let invalid = RequestProxySettings {
            enabled: true,
            url: "ftp://proxy.internal:21".to_string(),
            bypass: vec![],
        };

        assert!(
            pool.apply_persisted_request_proxy_settings(&invalid)
                .is_err()
        );
        assert!(pool.client(HttpClientProfile::Default).is_err());
        assert!(pool.git_blocking_client_builder().is_err());
        grant_user_endpoint(&pool, "http://localhost:11434/v1");
        assert!(
            pool.user_endpoint_client(
                HttpClientProfile::ChatCompletion,
                "http://localhost:11434/v1"
            )
            .is_ok()
        );

        pool.apply_request_proxy_settings(&RequestProxySettings::default())
            .unwrap();
        assert!(pool.client(HttpClientProfile::Default).is_ok());
    }

    #[test]
    fn clients_are_cached_per_profile() {
        let pool = pool();

        pool.client(HttpClientProfile::Default).unwrap();
        assert_eq!(pool.state.read().unwrap().clients.len(), 1);

        pool.client(HttpClientProfile::Default).unwrap();
        assert_eq!(pool.state.read().unwrap().clients.len(), 1);

        pool.client(HttpClientProfile::Tokenizer).unwrap();
        assert_eq!(pool.state.read().unwrap().clients.len(), 2);
    }

    #[test]
    fn apply_clears_cached_clients() {
        let pool = pool();

        pool.client(HttpClientProfile::Default).unwrap();
        assert_eq!(pool.state.read().unwrap().clients.len(), 1);

        let revision_before = pool.state.read().unwrap().revision;
        pool.apply_request_proxy_settings(&RequestProxySettings::default())
            .unwrap();

        let state = pool.state.read().unwrap();
        assert_eq!(state.clients.len(), 0);
        assert_eq!(state.revision, revision_before + 1);
    }

    #[test]
    fn granting_local_access_invalidates_cached_transports() {
        let pool = pool();
        pool.client(HttpClientProfile::Default).unwrap();
        let revision_before = pool.state.read().unwrap().revision;

        grant_user_endpoint(&pool, "http://localhost:11434/v1");

        let state = pool.state.read().unwrap();
        assert!(state.clients.is_empty());
        assert_eq!(state.revision, revision_before + 1);
    }

    #[test]
    fn client_with_revision_tracks_proxy_revision() {
        let pool = pool();

        let (_, initial_revision) = pool
            .client_with_revision(HttpClientProfile::ChatCompletionWebSocket)
            .unwrap();

        pool.apply_request_proxy_settings(&RequestProxySettings::default())
            .unwrap();

        let (_, next_revision) = pool
            .client_with_revision(HttpClientProfile::ChatCompletionWebSocket)
            .unwrap();

        assert_eq!(next_revision, initial_revision + 1);
    }

    #[test]
    fn apply_sets_and_clears_proxy() {
        let pool = pool();

        let enabled = RequestProxySettings {
            enabled: true,
            url: "http://127.0.0.1:7890".to_string(),
            bypass: vec![],
        };
        pool.apply_request_proxy_settings(&enabled).unwrap();
        assert!(matches!(
            pool.state.read().unwrap().proxy,
            super::RequestProxyState::Configured(_)
        ));

        pool.apply_request_proxy_settings(&RequestProxySettings::default())
            .unwrap();
        assert!(matches!(
            pool.state.read().unwrap().proxy,
            super::RequestProxyState::Disabled
        ));
    }

    #[test]
    fn git_blocking_builder_uses_product_user_agent() {
        let server = capture_server();
        let client = pool()
            .git_blocking_client_builder()
            .unwrap()
            .build()
            .unwrap();

        client.get(&server.url).send().unwrap();
        let request = server
            .requests
            .recv_timeout(Duration::from_secs(1))
            .expect("captured request");
        assert!(
            request
                .lines()
                .any(|line| line.eq_ignore_ascii_case("user-agent: TauriTavern/test"))
        );
        server.finish();
    }

    #[test]
    fn git_blocking_clients_snapshot_proxy_settings() {
        let first_proxy = capture_server();
        let second_proxy = capture_server();
        let pool = pool();
        pool.apply_request_proxy_settings(&RequestProxySettings {
            enabled: true,
            url: first_proxy.url.clone(),
            bypass: vec![],
        })
        .unwrap();
        let first_client = pool.git_blocking_client_builder().unwrap().build().unwrap();

        pool.apply_request_proxy_settings(&RequestProxySettings {
            enabled: true,
            url: second_proxy.url.clone(),
            bypass: vec![],
        })
        .unwrap();
        let second_client = pool.git_blocking_client_builder().unwrap().build().unwrap();

        first_client.get("http://git.invalid/first").send().unwrap();
        second_client
            .get("http://git.invalid/second")
            .send()
            .unwrap();
        let first_request = first_proxy
            .requests
            .recv_timeout(Duration::from_secs(1))
            .expect("first proxy request");
        let second_request = second_proxy
            .requests
            .recv_timeout(Duration::from_secs(1))
            .expect("second proxy request");
        assert!(first_request.starts_with("GET http://git.invalid/first HTTP/1.1"));
        assert!(second_request.starts_with("GET http://git.invalid/second HTTP/1.1"));
        first_proxy.finish();
        second_proxy.finish();
    }

    #[test]
    fn git_blocking_builder_honors_no_proxy() {
        let origin = capture_server();
        let (proxy_url, proxy_hit, proxy_handle) = proxy_probe(Duration::from_millis(150));
        let pool = pool();
        pool.apply_request_proxy_settings(&RequestProxySettings {
            enabled: true,
            url: proxy_url,
            bypass: vec!["127.0.0.1".to_string()],
        })
        .unwrap();
        let client = pool.git_blocking_client_builder().unwrap().build().unwrap();

        client.get(&origin.url).send().unwrap();
        let origin_request = origin
            .requests
            .recv_timeout(Duration::from_secs(1))
            .expect("origin request");
        assert!(origin_request.starts_with("GET / HTTP/1.1"));
        assert!(!proxy_hit.recv_timeout(Duration::from_secs(1)).unwrap());
        origin.finish();
        proxy_handle.join().expect("proxy probe thread");
    }

    #[test]
    fn git_blocking_builder_validates_server_certificates() {
        let pool = pool();
        let (config, root) = test_tls_config();

        let (untrusted_url, untrusted_request, untrusted_handle) = tls_server(Arc::clone(&config));
        let client = pool.git_blocking_client_builder().unwrap().build().unwrap();
        assert!(client.get(untrusted_url).send().is_err());
        assert!(
            !untrusted_request
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
        );
        untrusted_handle.join().expect("untrusted TLS server");

        let (trusted_url, trusted_request, trusted_handle) = tls_server(config);
        let client = pool
            .git_blocking_client_builder()
            .unwrap()
            .tls_certs_only([root])
            .build()
            .unwrap();
        assert!(
            client
                .get(trusted_url)
                .send()
                .unwrap()
                .status()
                .is_success()
        );
        assert!(
            trusted_request
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
        );
        trusted_handle.join().expect("trusted TLS server");
    }

    #[tokio::test]
    async fn approved_loopback_user_endpoint_forces_direct_client() {
        let origin = capture_server();
        let (proxy_url, proxy_hit, proxy_handle) = proxy_probe(Duration::from_millis(150));
        let pool = pool();
        pool.apply_request_proxy_settings(&RequestProxySettings {
            enabled: true,
            url: proxy_url,
            bypass: vec![],
        })
        .unwrap();
        assert!(
            pool.user_endpoint_client(HttpClientProfile::ChatCompletion, &origin.url)
                .is_err()
        );
        grant_user_endpoint(&pool, &origin.url);

        let client = pool
            .user_endpoint_client(HttpClientProfile::ChatCompletion, &origin.url)
            .unwrap();
        client.get(&origin.url).send().await.unwrap();

        assert!(origin.requests.recv_timeout(Duration::from_secs(1)).is_ok());
        assert!(!proxy_hit.recv_timeout(Duration::from_secs(1)).unwrap());
        origin.finish();
        proxy_handle.join().expect("proxy probe thread");
    }

    #[tokio::test]
    async fn approved_hostname_user_endpoint_honors_request_proxy() {
        let proxy = capture_server();
        let pool = pool();
        pool.apply_request_proxy_settings(&RequestProxySettings {
            enabled: true,
            url: proxy.url.clone(),
            bypass: vec![],
        })
        .unwrap();
        let endpoint = "http://provider.invalid/v1";
        grant_user_endpoint(&pool, endpoint);

        let client = pool
            .user_endpoint_client(HttpClientProfile::ChatCompletion, endpoint)
            .unwrap();
        client.get(endpoint).send().await.unwrap();

        let request = proxy
            .requests
            .recv_timeout(Duration::from_secs(1))
            .expect("proxy request");
        assert!(request.starts_with("GET http://provider.invalid/v1 HTTP/1.1"));
        proxy.finish();
    }

    #[tokio::test]
    async fn user_endpoint_client_follows_same_origin_redirects() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let redirect_handle = thread::spawn(move || {
            let (mut first, _peer) = listener.accept().expect("accept redirect request");
            let first_request = read_request_head(&first).expect("read redirect request");
            first
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("write redirect response");

            let (mut second, _peer) = listener.accept().expect("accept redirected request");
            let second_request = read_request_head(&second).expect("read redirected request");
            second
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("write final response");
            (first_request, second_request)
        });
        let pool = pool();
        grant_user_endpoint(&pool, &base_url);
        let client = pool
            .user_endpoint_client(HttpClientProfile::ChatCompletion, &base_url)
            .unwrap();

        let response = client
            .get(format!("{base_url}/start"))
            .send()
            .await
            .unwrap();
        let (first_request, second_request) = redirect_handle.join().expect("redirect server");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(first_request.starts_with("GET /start HTTP/1.1"));
        assert!(second_request.starts_with("GET /final HTTP/1.1"));
    }

    #[tokio::test]
    async fn user_endpoint_client_rejects_cross_origin_redirects() {
        let (target_url, target_hit, target_handle) = proxy_probe(Duration::from_millis(150));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
        let redirect_url = format!("http://{}", listener.local_addr().unwrap());
        let redirect_handle = thread::spawn(move || {
            let (mut stream, _peer) = listener.accept().expect("accept redirect request");
            read_request_head(&stream).expect("read redirect request");
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("write redirect response");
        });
        let pool = pool();
        grant_user_endpoint(&pool, &redirect_url);
        let client = pool
            .user_endpoint_client(HttpClientProfile::ChatCompletion, &redirect_url)
            .unwrap();

        let error = client.get(&redirect_url).send().await.unwrap_err();

        assert!(error.is_redirect());
        assert!(!target_hit.recv_timeout(Duration::from_secs(1)).unwrap());
        redirect_handle.join().expect("redirect server thread");
        target_handle.join().expect("redirect target thread");
    }
}
