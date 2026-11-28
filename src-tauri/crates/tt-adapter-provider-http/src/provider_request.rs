//! Client-free request construction shared by sending and inspection.

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use reqwest::{Client, Method, RequestBuilder, Url};
use tt_contracts::observability::{
    REDACTED, RequestDescriptionDto, RequestHeaderDto, RequestTransportDto,
};
use tt_domain::errors::DomainError;
use url::form_urlencoded;

pub(crate) const JSON: &str = "application/json";
pub(crate) const EVENT_STREAM: &str = "text/event-stream";

const CREDENTIAL_NAME_PARTS: [&str; 6] = ["auth", "cookie", "key", "password", "secret", "token"];

#[derive(Clone, PartialEq)]
pub(crate) struct ProviderRequest {
    method: Method,
    url: Url,
    headers: HeaderMap,
}

impl ProviderRequest {
    pub(crate) fn get(url: Url) -> Self {
        Self {
            method: Method::GET,
            url,
            headers: HeaderMap::new(),
        }
    }

    pub(crate) fn get_json(url: Url) -> Self {
        let mut request = Self::get(url);
        request
            .headers
            .insert(ACCEPT, HeaderValue::from_static(JSON));
        request
    }

    pub(crate) fn post_json(url: Url, accept: &'static str) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON));
        headers.insert(ACCEPT, HeaderValue::from_static(accept));
        Self {
            method: Method::POST,
            url,
            headers,
        }
    }

    pub(crate) fn header(&mut self, name: &str, value: &str) -> Result<(), DomainError> {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            DomainError::InvalidData(format!("Request header name is invalid: {name}"))
        })?;
        let value = HeaderValue::from_str(value).map_err(|_| {
            DomainError::InvalidData(format!("Request header {name} has an invalid value"))
        })?;
        self.headers.insert(header_name, value);
        Ok(())
    }

    pub(crate) fn query(&mut self, name: &str, value: &str) {
        self.url.query_pairs_mut().append_pair(name, value);
    }

    pub(crate) fn with_client(self, client: &Client) -> RequestBuilder {
        client.request(self.method, self.url).headers(self.headers)
    }

    /// Redacts values with credential names or matching `credentials`.
    /// Adds the default User-Agent only if absent; omits connection headers (Host/Content-Length).
    pub(crate) fn describe(
        &self,
        transport: RequestTransportDto,
        user_agent: &str,
        credentials: &[&str],
    ) -> RequestDescriptionDto {
        let is_credential = |name: &str, value: &str| {
            let value = value.trim();
            !value.is_empty()
                && (has_credential_name(name)
                    || credentials
                        .iter()
                        .any(|credential| credential.trim() == value))
        };

        let mut headers = self
            .headers
            .iter()
            .map(|(name, value)| {
                let value = String::from_utf8_lossy(value.as_bytes());
                RequestHeaderDto {
                    name: name.to_string(),
                    value: if is_credential(name.as_str(), &value) {
                        REDACTED.to_string()
                    } else {
                        value.into_owned()
                    },
                }
            })
            .collect::<Vec<_>>();
        if !self.headers.contains_key(USER_AGENT) {
            headers.push(RequestHeaderDto {
                name: USER_AGENT.to_string(),
                value: user_agent.to_string(),
            });
        }

        let mut url = self.url.clone();
        if transport == RequestTransportDto::Websocket {
            let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
            url.set_scheme(scheme)
                .expect("an http URL takes a WebSocket scheme");
        }
        if let Some(query) = self.url.query() {
            // Preserve non-credential parameters' original encoding.
            let query = query
                .split('&')
                .map(
                    |pair| match form_urlencoded::parse(pair.as_bytes()).next() {
                        Some((name, value)) if is_credential(&name, &value) => {
                            let name = pair.split_once('=').map_or(pair, |(name, _)| name);
                            format!("{name}={REDACTED}")
                        }
                        _ => pair.to_string(),
                    },
                )
                .collect::<Vec<_>>()
                .join("&");
            url.set_query(Some(&query));
        }

        RequestDescriptionDto {
            transport,
            method: self.method.to_string(),
            url: url.into(),
            headers,
        }
    }

    #[cfg(test)]
    pub(crate) fn url(&self) -> &Url {
        &self.url
    }

    #[cfg(test)]
    pub(crate) fn header_value(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

fn has_credential_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    CREDENTIAL_NAME_PARTS.iter().any(|part| name.contains(part))
}
