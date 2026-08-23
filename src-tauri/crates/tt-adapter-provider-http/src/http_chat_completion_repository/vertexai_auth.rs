use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;

use super::HttpChatCompletionRepository;

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const GOOGLE_OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const JWT_BEARER_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
const JWT_LIFETIME_SECONDS: u64 = 60 * 60;
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct CachedAccessToken {
    value: String,
    refresh_at: Instant,
}

#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
    private_key_id: Option<String>,
}

#[derive(Serialize)]
struct ServiceAccountClaims<'a> {
    iss: &'a str,
    scope: &'static str,
    aud: &'static str,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: String,
    expires_in: u64,
}

static ACCESS_TOKEN_CACHE: OnceLock<RwLock<HashMap<String, CachedAccessToken>>> = OnceLock::new();

pub(crate) async fn get_service_account_access_token(
    http_clients: &HttpClientPool,
    service_account_json: &str,
) -> Result<String, DomainError> {
    get_service_account_access_token_at(http_clients, service_account_json, GOOGLE_OAUTH_TOKEN_URL)
        .await
}

async fn get_service_account_access_token_at(
    http_clients: &HttpClientPool,
    service_account_json: &str,
    token_url: &str,
) -> Result<String, DomainError> {
    let cache_key = sha256_hex(service_account_json);
    let cached = {
        let cache = access_token_cache().read().await;
        cache
            .get(&cache_key)
            .filter(|token| token.refresh_at > Instant::now())
            .cloned()
    };
    if let Some(cached) = cached {
        return Ok(cached.value);
    }

    // ponytail: concurrent misses may refresh twice; add per-credential singleflight only if
    // OAuth traffic becomes measurable.
    let service_account_key = serde_json::from_str::<ServiceAccountKey>(service_account_json)
        .map_err(|error| {
            DomainError::InvalidData(format!(
                "Vertex AI service account JSON parse failed: {error}"
            ))
        })?;
    if service_account_key.client_email.trim().is_empty() {
        return Err(DomainError::InvalidData(
            "Vertex AI service account JSON is missing client_email".to_string(),
        ));
    }

    let assertion = service_account_assertion(&service_account_key)?;
    let response = request_access_token(http_clients, token_url, &assertion).await?;
    if response.access_token.trim().is_empty() {
        return Err(DomainError::InternalError(
            "Vertex AI access token response is missing token".to_string(),
        ));
    }

    let refresh_after =
        Duration::from_secs(response.expires_in).saturating_sub(TOKEN_REFRESH_MARGIN);
    let cached = CachedAccessToken {
        value: response.access_token,
        refresh_at: Instant::now() + refresh_after,
    };
    access_token_cache()
        .write()
        .await
        .insert(cache_key, cached.clone());
    Ok(cached.value)
}

fn access_token_cache() -> &'static RwLock<HashMap<String, CachedAccessToken>> {
    ACCESS_TOKEN_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn service_account_assertion(
    service_account_key: &ServiceAccountKey,
) -> Result<String, DomainError> {
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "System clock cannot create Vertex AI service account assertion: {error}"
            ))
        })?
        .as_secs();
    let claims = ServiceAccountClaims {
        iss: service_account_key.client_email.trim(),
        scope: CLOUD_PLATFORM_SCOPE,
        aud: GOOGLE_OAUTH_TOKEN_URL,
        iat: issued_at,
        exp: issued_at + JWT_LIFETIME_SECONDS,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = service_account_key.private_key_id.clone();
    let key =
        EncodingKey::from_rsa_pem(service_account_key.private_key.as_bytes()).map_err(|error| {
            DomainError::InvalidData(format!(
                "Vertex AI service account private key is invalid: {error}"
            ))
        })?;

    encode(&header, &claims, &key).map_err(|error| {
        DomainError::InvalidData(format!(
            "Vertex AI service account assertion signing failed: {error}"
        ))
    })
}

async fn request_access_token(
    http_clients: &HttpClientPool,
    token_url: &str,
    assertion: &str,
) -> Result<AccessTokenResponse, DomainError> {
    let response = http_clients
        .client(HttpClientProfile::ProviderAuthentication)?
        .post(token_url)
        .form(&[
            ("grant_type", JWT_BEARER_GRANT_TYPE),
            ("assertion", assertion),
        ])
        .send()
        .await
        .map_err(|error| {
            HttpChatCompletionRepository::map_transport_error(
                "Vertex AI service account access token request failed",
                error,
            )
        })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Google OAuth",
            response,
            "Vertex AI service account access token request failed",
        )
        .await);
    }

    response
        .json::<AccessTokenResponse>()
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Vertex AI access token response is invalid: {error}"
            ))
        })
}

fn sha256_hex(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(input.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};

    use serde_json::json;
    use tt_domain::models::settings::RequestProxySettings;

    use super::*;

    const TEST_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDJETqse41HRBsc
7cfcq3ak4oZWFCoZlcic525A3FfO4qW9BMtRO/iXiyCCHn8JhiL9y8j5JdVP2Q9Z
IpfElcFd3/guS9w+5RqQGgCR+H56IVUyHZWtTJbKPcwWXQdNUX0rBFcsBzCRESJL
eelOEdHIjG7LRkx5l/FUvlqsyHDVJEQsHwegZ8b8C0fz0EgT2MMEdn10t6Ur1rXz
jMB/wvCg8vG8lvciXmedyo9xJ8oMOh0wUEgxziVDMMovmC+aJctcHUAYubwoGN8T
yzcvnGqL7JSh36Pwy28iPzXZ2RLhAyJFU39vLaHdljwthUaupldlNyCfa6Ofy4qN
ctlUPlN1AgMBAAECggEAdESTQjQ70O8QIp1ZSkCYXeZjuhj081CK7jhhp/4ChK7J
GlFQZMwiBze7d6K84TwAtfQGZhQ7km25E1kOm+3hIDCoKdVSKch/oL54f/BK6sKl
qlIzQEAenho4DuKCm3I4yAw9gEc0DV70DuMTR0LEpYyXcNJY3KNBOTjN5EYQAR9s
2MeurpgK2MdJlIuZaIbzSGd+diiz2E6vkmcufJLtmYUT/k/ddWvEtz+1DnO6bRHh
xuuDMeJA/lGB/EYloSLtdyCF6sII6C6slJJtgfb0bPy7l8VtL5iDyz46IKyzdyzW
tKAn394dm7MYR1RlUBEfqFUyNK7C+pVMVoTwCC2V4QKBgQD64syfiQ2oeUlLYDm4
CcKSP3RnES02bcTyEDFSuGyyS1jldI4A8GXHJ/lG5EYgiYa1RUivge4lJrlNfjyf
dV230xgKms7+JiXqag1FI+3mqjAgg4mYiNjaao8N8O3/PD59wMPeWYImsWXNyeHS
55rUKiHERtCcvdzKl4u35ZtTqQKBgQDNKnX2bVqOJ4WSqCgHRhOm386ugPHfy+8j
m6cicmUR46ND6ggBB03bCnEG9OtGisxTo/TuYVRu3WP4KjoJs2LD5fwdwJqpgtHl
yVsk45Y1Hfo+7M6lAuR8rzCi6kHHNb0HyBmZjysHWZsn79ZM+sQnLpgaYgQGRbKV
DZWlbw7g7QKBgQCl1u+98UGXAP1jFutwbPsx40IVszP4y5ypCe0gqgon3UiY/G+1
zTLp79GGe/SjI2VpQ7AlW7TI2A0bXXvDSDi3/5Dfya9ULnFXv9yfvH1QwWToySpW
Kvd1gYSoiX84/WCtjZOr0e0HmLIb0vw0hqZA4szJSqoxQgvF22EfIWaIaQKBgQCf
34+OmMYw8fEvSCPxDxVvOwW2i7pvV14hFEDYIeZKW2W1HWBhVMzBfFB5SE8yaCQy
pRfOzj9aKOCm2FjjiErVNpkQoi6jGtLvScnhZAt/lr2TXTrl8OwVkPrIaN0bG/AS
aUYxmBPCpXu3UjhfQiWqFq/mFyzlqlgvuCc9g95HPQKBgAscKP8mLxdKwOgX8yFW
GcZ0izY/30012ajdHY+/QK5lsMoxTnn0skdS+spLxaS5ZEO4qvPVb8RAoCkWMMal
2pOhmquJQVDPDLuZHdrIiKiDM20dy9sMfHygWcZjQ4WSxf/J7T9canLZIXFhHAZT
3wc9h4G8BBCtWN2TN/LsGZdB
-----END PRIVATE KEY-----"#;

    #[tokio::test]
    async fn token_exchange_uses_updated_request_proxy() {
        let first_proxy = capture_proxy("502 Bad Gateway", "proxy unavailable");
        let second_proxy = capture_proxy(
            "200 OK",
            r#"{"access_token":"access-token","expires_in":3600}"#,
        );
        let pool = HttpClientPool::new("TauriTavern/test");
        let service_account_json = json!({
            "client_email": "proxy-test@example.test",
            "private_key": TEST_PRIVATE_KEY,
            "private_key_id": "test-key"
        })
        .to_string();

        pool.apply_request_proxy_settings(&proxy_settings(&first_proxy.url))
            .unwrap();
        get_service_account_access_token_at(
            &pool,
            &service_account_json,
            "http://oauth.invalid/token",
        )
        .await
        .unwrap_err();
        let first_request = first_proxy.request();

        pool.apply_request_proxy_settings(&proxy_settings(&second_proxy.url))
            .unwrap();
        let token = get_service_account_access_token_at(
            &pool,
            &service_account_json,
            "http://oauth.invalid/token",
        )
        .await
        .unwrap();
        let second_request = second_proxy.request();

        assert_eq!(token, "access-token");
        for request in [&first_request, &second_request] {
            assert!(request.starts_with("POST http://oauth.invalid/token HTTP/1.1\r\n"));
            assert!(request.contains("grant_type="));
            assert!(request.contains("assertion="));
        }

        pool.block_requests_for_invalid_proxy();
        assert_eq!(
            get_service_account_access_token_at(
                &pool,
                &service_account_json,
                "http://oauth.invalid/token",
            )
            .await
            .unwrap(),
            "access-token"
        );

        first_proxy.finish();
        second_proxy.finish();
    }

    fn proxy_settings(url: &str) -> RequestProxySettings {
        RequestProxySettings {
            enabled: true,
            url: url.to_string(),
            bypass: Vec::new(),
        }
    }

    struct CaptureProxy {
        url: String,
        request_rx: Receiver<String>,
        handle: JoinHandle<()>,
    }

    impl CaptureProxy {
        fn request(&self) -> String {
            self.request_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("proxy request")
        }

        fn finish(self) {
            self.handle.join().expect("proxy thread");
        }
    }

    fn capture_proxy(status: &'static str, body: &'static str) -> CaptureProxy {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind proxy");
        let url = format!("http://{}", listener.local_addr().expect("proxy address"));
        let (request_tx, request_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept proxy request");
            let mut reader = BufReader::new(stream.try_clone().expect("clone proxy stream"));
            let mut request = String::new();
            let mut content_length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request header");
                if line == "\r\n" {
                    request.push_str(&line);
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse().expect("content length");
                }
                request.push_str(&line);
            }
            let mut request_body = vec![0; content_length];
            reader
                .read_exact(&mut request_body)
                .expect("read request body");
            request.push_str(&String::from_utf8(request_body).expect("UTF-8 request body"));
            request_tx.send(request).expect("report proxy request");

            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write proxy response");
        });

        CaptureProxy {
            url,
            request_rx,
            handle,
        }
    }
}
