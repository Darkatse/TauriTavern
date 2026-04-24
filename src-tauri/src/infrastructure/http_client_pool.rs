use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::RwLock;
use std::time::Duration;

use reqwest::redirect::Policy;
use reqwest::{Client, NoProxy, Proxy};

use crate::domain::errors::DomainError;
use crate::domain::models::settings::RequestProxySettings;
use crate::infrastructure::http_client::build_http_client;

pub const CHAT_COMPLETION_CONNECT_TIMEOUT: Duration = Duration::from_secs(3 * 60);
pub const CHAT_COMPLETION_NON_STREAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const TOKENIZER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const TOKENIZER_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const IMAGE_GENERATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const TRANSLATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const TRANSLATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpClientProfile {
    Default,
    Download,
    Tokenizer,
    ChatCompletion,
    ChatCompletionStream,
    ImageGeneration,
    Translation,
}

#[derive(Default)]
struct HttpClientPoolState {
    revision: u64,
    proxy: Option<Proxy>,
    clients: HashMap<HttpClientProfile, Client>,
}

pub struct HttpClientPool {
    state: RwLock<HttpClientPoolState>,
}

impl Default for HttpClientPool {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClientPool {
    pub fn new() -> Self {
        Self {
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
        loop {
            let (revision, proxy) = {
                let state = self.state.read().unwrap();
                if let Some(client) = state.clients.get(&profile) {
                    return Ok(client.clone());
                }

                (state.revision, state.proxy.clone())
            };

            let client = build_profile_client(profile, proxy)?;

            let mut state = self.state.write().unwrap();
            if state.revision != revision {
                continue;
            }

            match state.clients.entry(profile) {
                Entry::Occupied(entry) => return Ok(entry.get().clone()),
                Entry::Vacant(entry) => {
                    entry.insert(client.clone());
                    return Ok(client);
                }
            }
        }
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
        HttpClientProfile::ImageGeneration => {
            builder.connect_timeout(IMAGE_GENERATION_CONNECT_TIMEOUT)
        }
        HttpClientProfile::Translation => builder
            .connect_timeout(TRANSLATION_CONNECT_TIMEOUT)
            .timeout(TRANSLATION_REQUEST_TIMEOUT),
    };

    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
    }

    build_http_client(builder).map_err(|error| {
        DomainError::InternalError(format!("Failed to build HTTP client: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{HttpClientPool, HttpClientProfile};
    use crate::domain::models::settings::RequestProxySettings;

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
            bypass: vec!["localhost".to_string()],
        };

        HttpClientPool::validate_request_proxy_settings(&settings).unwrap();
    }

    #[test]
    fn clients_are_cached_per_profile() {
        let pool = HttpClientPool::new();

        pool.client(HttpClientProfile::Default).unwrap();
        assert_eq!(pool.state.read().unwrap().clients.len(), 1);

        pool.client(HttpClientProfile::Default).unwrap();
        assert_eq!(pool.state.read().unwrap().clients.len(), 1);

        pool.client(HttpClientProfile::Tokenizer).unwrap();
        assert_eq!(pool.state.read().unwrap().clients.len(), 2);
    }

    #[test]
    fn apply_clears_cached_clients() {
        let pool = HttpClientPool::new();

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
    fn apply_sets_and_clears_proxy() {
        let pool = HttpClientPool::new();

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
}
