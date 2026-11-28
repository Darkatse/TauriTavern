use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderMap, HeaderName, LOCATION};
use reqwest::{Response, StatusCode};
use tokio::io::{AsyncWriteExt, BufWriter};
use url::Url;

use tt_adapter_http::{HttpClientPool, HttpClientProfile, upstream};
use tt_domain::errors::DomainError;
use tt_ports::external_import::{
    DownloadByteLimit, DownloadSources, DownloadedFile, ExternalImportDownloader,
};

const MAX_REDIRECTS: usize = 5;

pub struct HttpExternalImportDownloader {
    http_clients: Arc<HttpClientPool>,
}

impl HttpExternalImportDownloader {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    async fn get(
        &self,
        mut url: Url,
        sources: &dyn DownloadSources,
    ) -> Result<Response, DomainError> {
        let client = self.http_clients.client(HttpClientProfile::Download)?;
        for _ in 0..=MAX_REDIRECTS {
            sources.allow(&url).await?;
            let response = client
                .get(url)
                .send()
                .await
                .map_err(upstream::request_failure)?;
            match redirect_target(&response)? {
                Some(next) => url = next,
                None if response.status().is_success() => return Ok(response),
                None => {
                    return Err(DomainError::InternalError(format!(
                        "{} responded with HTTP {}",
                        response.url().origin().ascii_serialization(),
                        response.status()
                    )));
                }
            }
        }
        Err(DomainError::InternalError(format!(
            "Download exceeded {MAX_REDIRECTS} redirects"
        )))
    }
}

#[async_trait]
impl ExternalImportDownloader for HttpExternalImportDownloader {
    async fn fetch_bytes(
        &self,
        url: Url,
        limit: DownloadByteLimit,
        sources: &dyn DownloadSources,
    ) -> Result<Vec<u8>, DomainError> {
        let response = self.get(url, sources).await?;
        if response
            .content_length()
            .is_some_and(|length| length > limit.max_bytes as u64)
        {
            return Err(limit_error(limit));
        }

        read_limited_response(response, limit).await
    }

    async fn fetch_to_file(
        &self,
        url: Url,
        path: &Path,
        sources: &dyn DownloadSources,
    ) -> Result<DownloadedFile, DomainError> {
        let response = self.get(url, sources).await?;
        let content_type = header_string(response.headers(), CONTENT_TYPE);
        let content_disposition = header_string(response.headers(), CONTENT_DISPOSITION);
        let file = tokio::fs::File::create(path)
            .await
            .map_err(internal_error)?;
        let endpoint = response.url().clone();
        let mut writer = BufWriter::with_capacity(64 * 1024, file);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream
            .try_next()
            .await
            .map_err(|error| upstream::body_failure(error, &endpoint))?
        {
            writer.write_all(&chunk).await.map_err(internal_error)?;
        }
        writer.flush().await.map_err(internal_error)?;
        Ok(DownloadedFile {
            content_type,
            content_disposition,
        })
    }
}

fn redirect_target(response: &Response) -> Result<Option<Url>, DomainError> {
    let status = response.status();
    if !matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    ) {
        return Ok(None);
    }
    response
        .headers()
        .get(LOCATION)
        .and_then(|location| location.to_str().ok())
        .and_then(|location| response.url().join(location).ok())
        .filter(|target| matches!(target.scheme(), "http" | "https"))
        .map(Some)
        .ok_or_else(|| {
            DomainError::InternalError(format!(
                "{} responded with HTTP {status} without a usable Location",
                response.url().origin().ascii_serialization()
            ))
        })
}

async fn read_limited_response(
    response: reqwest::Response,
    limit: DownloadByteLimit,
) -> Result<Vec<u8>, DomainError> {
    let mut bytes = Vec::new();
    let endpoint = response.url().clone();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream
        .try_next()
        .await
        .map_err(|error| upstream::body_failure(error, &endpoint))?
    {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| limit_error(limit))?;
        if next_len > limit.max_bytes {
            return Err(limit_error(limit));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn header_string(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn limit_error(limit: DownloadByteLimit) -> DomainError {
    DomainError::InvalidData(format!(
        "{} must be <= {} bytes",
        limit.label, limit.max_bytes
    ))
}

fn internal_error(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Mutex, mpsc};
    use std::thread;
    use std::time::Duration;

    use super::*;

    struct OneOrigin {
        origin: String,
        asked: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl DownloadSources for OneOrigin {
        async fn allow(&self, url: &Url) -> Result<(), DomainError> {
            self.asked.lock().unwrap().push(url.to_string());
            if url.origin().ascii_serialization() == self.origin {
                Ok(())
            } else {
                Err(DomainError::Cancelled("refused".to_string()))
            }
        }
    }

    fn read_request_line(stream: &TcpStream) -> String {
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let mut header = String::new();
        while reader.read_line(&mut header).unwrap() > 2 {
            header.clear();
        }
        request_line
    }

    #[tokio::test]
    async fn each_redirect_is_allowed_before_it_is_requested() {
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        other.set_nonblocking(true).unwrap();
        let other_url = format!("http://{}/card.png", other.local_addr().unwrap());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let redirects = ["/moved".to_string(), other_url.clone()];
        let (requests, received) = mpsc::channel();
        thread::spawn(move || {
            for location in redirects {
                let (mut stream, _peer) = listener.accept().unwrap();
                requests.send(read_request_line(&stream)).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        });
        let downloader =
            HttpExternalImportDownloader::new(Arc::new(HttpClientPool::new("TauriTavern/test")));
        let sources = OneOrigin {
            origin: origin.clone(),
            asked: Mutex::default(),
        };

        let error = downloader
            .fetch_bytes(
                Url::parse(&format!("{origin}/start")).unwrap(),
                DownloadByteLimit {
                    label: "test",
                    max_bytes: 16,
                },
                &sources,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, DomainError::Cancelled(_)));
        let requested: Vec<String> = (0..2)
            .map(|_| received.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        assert_eq!(
            requested,
            ["GET /start HTTP/1.1\r\n", "GET /moved HTTP/1.1\r\n"]
        );
        assert_eq!(
            *sources.asked.lock().unwrap(),
            [
                format!("{origin}/start"),
                format!("{origin}/moved"),
                other_url
            ]
        );
        assert!(other.accept().is_err(), "refused URL was requested");
    }
}
