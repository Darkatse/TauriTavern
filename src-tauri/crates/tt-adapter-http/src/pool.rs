use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::RwLock;
use std::time::Duration;

use reqwest::blocking::{Client as BlockingClient, ClientBuilder as BlockingClientBuilder};
use reqwest::redirect::Policy;
use reqwest::{Client, NoProxy, Proxy};
use tt_domain::errors::DomainError;
use tt_domain::models::settings::RequestProxySettings;
use tt_ports::settings::RequestProxyRuntime;

use crate::client::{build_http_client, configure_blocking_http_client};

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
}

#[derive(Default)]
struct HttpClientPoolState {
    revision: u64,
    proxy: Option<Proxy>,
    clients: HashMap<HttpClientProfile, Client>,
}

pub struct HttpClientPool {
    product_user_agent: String,
    state: RwLock<HttpClientPoolState>,
}

impl HttpClientPool {
    pub fn new(product_user_agent: impl Into<String>) -> Self {
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

        let mut state = self.state.write().unwrap();
        state.proxy = proxy;
        state.clients.clear();
        state.revision += 1;
        Ok(())
    }

    pub fn client(&self, profile: HttpClientProfile) -> Result<Client, DomainError> {
        self.client_with_revision(profile)
            .map(|(client, _revision)| client)
    }

    pub fn client_with_revision(
        &self,
        profile: HttpClientProfile,
    ) -> Result<(Client, u64), DomainError> {
        loop {
            let (revision, proxy) = {
                let state = self.state.read().unwrap();
                if let Some(client) = state.clients.get(&profile) {
                    return Ok((client.clone(), state.revision));
                }

                (state.revision, state.proxy.clone())
            };

            let client = build_profile_client(profile, proxy, &self.product_user_agent)?;

            let mut state = self.state.write().unwrap();
            if state.revision != revision {
                continue;
            }

            match state.clients.entry(profile) {
                Entry::Occupied(entry) => return Ok((entry.get().clone(), state.revision)),
                Entry::Vacant(entry) => {
                    entry.insert(client.clone());
                    return Ok((client, state.revision));
                }
            }
        }
    }

    pub fn git_blocking_client_builder(&self) -> BlockingClientBuilder {
        let proxy = self.state.read().unwrap().proxy.clone();
        let mut builder = BlockingClient::builder()
            .no_proxy()
            .connect_timeout(GIT_CONNECT_TIMEOUT)
            .timeout(GIT_REQUEST_TIMEOUT);

        if let Some(proxy) = proxy {
            builder = builder.proxy(proxy);
        }

        configure_blocking_http_client(builder, &self.product_user_agent)
    }
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

fn proxy_from_settings(settings: &RequestProxySettings) -> Result<Option<Proxy>, DomainError> {
    if !settings.enabled {
        return Ok(None);
    }

    let url = settings.url.trim();
    if url.is_empty() {
        return Err(DomainError::InvalidData(
            "Request proxy URL is required".to_string(),
        ));
    }

    let mut proxy = Proxy::all(url)
        .map_err(|error| DomainError::InvalidData(format!("Invalid request proxy URL: {error}")))?;

    let bypass = normalized_bypass_csv(&settings.bypass);
    if !bypass.is_empty() {
        proxy = proxy.no_proxy(NoProxy::from_string(&bypass));
    }

    Ok(Some(proxy))
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
    proxy: Option<Proxy>,
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
    };

    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
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
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{ServerConfig, ServerConnection, StreamOwned};
    use tt_domain::models::settings::RequestProxySettings;

    const TEST_USER_AGENT: &str = "TauriTavern/test";

    fn pool() -> HttpClientPool {
        HttpClientPool::new(TEST_USER_AGENT)
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
        let CertifiedKey { cert, key_pair } =
            generate_simple_self_signed(["127.0.0.1".to_string()]).expect("test certificate");
        let certificate = cert.der().clone();
        let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
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
    fn http_proxy_url_is_accepted() {
        let settings = RequestProxySettings {
            enabled: true,
            url: "http://127.0.0.1:7890".to_string(),
            bypass: vec!["localhost".to_string()],
        };

        HttpClientPool::validate_request_proxy_settings(&settings).unwrap();
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
        assert!(pool.state.read().unwrap().proxy.is_some());

        pool.apply_request_proxy_settings(&RequestProxySettings::default())
            .unwrap();
        assert!(pool.state.read().unwrap().proxy.is_none());
    }

    #[test]
    fn git_blocking_builder_uses_product_user_agent() {
        let server = capture_server();
        let client = pool().git_blocking_client_builder().build().unwrap();

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
        let first_client = pool.git_blocking_client_builder().build().unwrap();

        pool.apply_request_proxy_settings(&RequestProxySettings {
            enabled: true,
            url: second_proxy.url.clone(),
            bypass: vec![],
        })
        .unwrap();
        let second_client = pool.git_blocking_client_builder().build().unwrap();

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
        let client = pool.git_blocking_client_builder().build().unwrap();

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
        let (config, root) = test_tls_config();

        let (untrusted_url, untrusted_request, untrusted_handle) = tls_server(Arc::clone(&config));
        let client = pool().git_blocking_client_builder().build().unwrap();
        assert!(client.get(untrusted_url).send().is_err());
        assert!(
            !untrusted_request
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
        );
        untrusted_handle.join().expect("untrusted TLS server");

        let (trusted_url, trusted_request, trusted_handle) = tls_server(config);
        let client = pool()
            .git_blocking_client_builder()
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
}
