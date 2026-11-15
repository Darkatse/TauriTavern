use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Response, StatusCode, Url};

use tt_adapter_http::{HttpClientPool, HttpClientProfile, upstream};
use tt_domain::errors::DomainError;
use tt_ports::byte_reader::ByteReader;
use tt_ports::repositories::cors_proxy_repository::{
    CorsProxyRepository, CorsProxyRequest, CorsProxyResponse, Headers,
};

pub struct HttpCorsProxyRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpCorsProxyRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }
}

#[async_trait]
impl CorsProxyRepository for HttpCorsProxyRepository {
    async fn send(&self, request: CorsProxyRequest) -> Result<CorsProxyResponse, DomainError> {
        let client = self
            .http_clients
            .user_endpoint_client(HttpClientProfile::CorsProxy, &request.endpoint)?;
        let method = Method::from_bytes(request.method.as_bytes()).map_err(|_| {
            DomainError::InvalidData(format!("Invalid CORS proxy method: {}", request.method))
        })?;

        let mut headers = header_map(request.headers)?;
        // Fetch states an empty POST or PUT body; hyper omits Content-Length for every empty body.
        if request.body.is_empty() && matches!(method, Method::POST | Method::PUT) {
            headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
        }
        let response = client
            .request(method.clone(), request.url)
            .headers(headers)
            .body(request.body)
            .send()
            .await
            .map_err(upstream::request_failure)?;

        let status = response.status();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
            .collect();
        let body = has_content(&method, status)
            .then(|| Box::new(ResponseBody::new(response)) as Box<dyn ByteReader>);
        Ok(CorsProxyResponse {
            status: status.as_u16(),
            headers,
            body,
        })
    }
}

fn header_map(headers: Headers) -> Result<HeaderMap, DomainError> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            DomainError::InvalidData(format!("Invalid CORS proxy header name: {name}"))
        })?;
        let value = HeaderValue::from_bytes(&value).map_err(|_| {
            DomainError::InvalidData(format!("Invalid value for CORS proxy header {name}"))
        })?;
        map.append(name, value);
    }
    Ok(map)
}

/// HTTP gives responses to HEAD, and 204, 205 and 304 responses, no content.
fn has_content(method: &Method, status: StatusCode) -> bool {
    *method != Method::HEAD
        && !matches!(
            status,
            StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
        )
}

/// Each read returns what has arrived, so a streamed body reaches the page as the target sends it.
struct ResponseBody {
    endpoint: Url,
    response: Response,
    pending: Bytes,
}

impl ResponseBody {
    fn new(response: Response) -> Self {
        Self {
            endpoint: response.url().clone(),
            response,
            pending: Bytes::new(),
        }
    }
}

#[async_trait]
impl ByteReader for ResponseBody {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, DomainError> {
        while self.pending.is_empty() {
            match self
                .response
                .chunk()
                .await
                .map_err(|error| upstream::body_failure(error, &self.endpoint))?
            {
                Some(chunk) => self.pending = chunk,
                None => return Ok(0),
            }
        }
        let count = self.pending.len().min(buffer.len());
        buffer[..count].copy_from_slice(&self.pending.split_to(count));
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;
    use tt_ports::user_endpoint_access::UserEndpointGrantRuntime;

    use super::*;

    #[tokio::test]
    async fn forwards_the_request_and_streams_the_response_unchanged() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (release_rest, rest_released) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Type: audio/mpeg\r\nX-Trace: \xe9\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n\x00\xff\x01\x02\r\n")
                .await
                .unwrap();
            rest_released.await.unwrap();
            stream
                .write_all(b"2\r\n\xfe\x80\r\n0\r\n\r\n")
                .await
                .unwrap();
            request
        });

        let pool = HttpClientPool::new("TauriTavern/test");
        let endpoint = format!("{origin}/");
        pool.allow_user_endpoints(std::slice::from_ref(&endpoint));
        let response = HttpCorsProxyRepository::new(Arc::new(pool))
            .send(CorsProxyRequest {
                endpoint,
                url: format!("{origin}/v1/tts?format=mp3").parse().unwrap(),
                method: "POST".to_string(),
                headers: vec![
                    ("content-type".to_string(), b"application/msgpack".to_vec()),
                    ("x-voice".to_string(), vec![0xe9]),
                ],
                body: vec![0x00, 0xff, 0xfe, 0x80],
            })
            .await
            .unwrap();

        assert_eq!(response.status, 201);
        assert!(
            response
                .headers
                .contains(&("content-type".to_string(), b"audio/mpeg".to_vec()))
        );
        assert!(
            response
                .headers
                .contains(&("x-trace".to_string(), vec![0xe9]))
        );

        let mut body = response.body.unwrap();
        let first = tokio::time::timeout(Duration::from_secs(5), read_bytes(&mut body, 4))
            .await
            .expect("bytes reach the reader before the body ends");
        assert_eq!(first, [0x00, 0xff, 0x01, 0x02]);
        release_rest.send(()).unwrap();
        assert_eq!(read_bytes(&mut body, usize::MAX).await, [0xfe, 0x80]);

        let request = server.await.unwrap();
        for expected in [
            &b"POST /v1/tts?format=mp3 HTTP/1.1\r\n"[..],
            b"\r\ncontent-type: application/msgpack\r\n",
            b"\r\nx-voice: \xe9\r\n",
            b"\r\n\r\n\x00\xff\xfe\x80",
        ] {
            assert!(
                request
                    .windows(expected.len())
                    .any(|window| window == expected),
                "{}",
                String::from_utf8_lossy(expected)
            );
        }
    }

    async fn read_bytes(body: &mut Box<dyn ByteReader>, limit: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 64];
        while bytes.len() < limit {
            let read = body.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        bytes
    }

    async fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "client closed the connection early");
            request.extend_from_slice(&buffer[..read]);
            let Some(head_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&request[..head_end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .map_or(0, |value| value.trim().parse::<usize>().unwrap());
            if request.len() >= head_end + 4 + length {
                return request;
            }
        }
    }
}
