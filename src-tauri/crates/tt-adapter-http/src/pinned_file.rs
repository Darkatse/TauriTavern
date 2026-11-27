use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use reqwest::header::RANGE;
use reqwest::{Response, StatusCode};
use sha2::{Digest, Sha256};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::Mutex;
use tt_domain::errors::DomainError;

use crate::{HttpClientPool, HttpClientProfile, upstream};

const IO_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedFile {
    pub url: &'static str,
    pub sha256: &'static str,
}

impl PinnedFile {
    fn partial_name(self) -> String {
        format!("{}.part", self.sha256)
    }
}

/// A digest-named file is immutable and appears only after SHA-256 verification.
pub struct PinnedFileCache {
    dir: PathBuf,
    http_clients: Arc<HttpClientPool>,
    writer: Mutex<()>,
}

impl PinnedFileCache {
    pub fn new(dir: PathBuf, http_clients: Arc<HttpClientPool>) -> Self {
        Self {
            dir,
            http_clients,
            writer: Mutex::new(()),
        }
    }

    pub async fn get(&self, file: PinnedFile) -> Result<PathBuf, DomainError> {
        let path = self.dir.join(file.sha256);
        if exists(&path).await? {
            return Ok(path);
        }
        let _writer = self.writer.lock().await;
        if !exists(&path).await? {
            self.download(file, &path).await?;
        }
        Ok(path)
    }

    /// Removes everything in the directory except `files` and their unfinished downloads.
    pub async fn keep_only(&self, files: &[PinnedFile]) -> Result<(), DomainError> {
        let kept = files
            .iter()
            .flat_map(|file| [OsString::from(file.sha256), file.partial_name().into()])
            .collect::<HashSet<_>>();
        let _writer = self.writer.lock().await;
        let mut entries = match fs::read_dir(&self.dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(file_error("read", &self.dir, error)),
        };
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| file_error("read", &self.dir, error))?
        {
            if kept.contains(&entry.file_name()) {
                continue;
            }
            let path = entry.path();
            let removed = match entry.file_type().await {
                Ok(file_type) if file_type.is_dir() => fs::remove_dir_all(&path).await,
                Ok(_) => fs::remove_file(&path).await,
                Err(error) => Err(error),
            };
            removed.map_err(|error| file_error("remove", &path, error))?;
        }
        Ok(())
    }

    async fn download(&self, file: PinnedFile, path: &Path) -> Result<(), DomainError> {
        fs::create_dir_all(&self.dir)
            .await
            .map_err(|error| file_error("create", &self.dir, error))?;
        let partial_path = self.dir.join(file.partial_name());
        let mut partial = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&partial_path)
            .await
            .map_err(|error| file_error("open", &partial_path, error))?;
        let mut digest = Sha256::new();
        // Hashing the prefix leaves the cursor at the resume offset.
        let received = digest_file(&mut partial, &mut digest)
            .await
            .map_err(|error| file_error("read", &partial_path, error))?;

        let mut request = self
            .http_clients
            .client(HttpClientProfile::PinnedFile)?
            .get(file.url);
        if received > 0 {
            request = request.header(RANGE, format!("bytes={received}-"));
        }
        let response = request.send().await.map_err(upstream::request_failure)?;
        let rest = match response.status() {
            StatusCode::PARTIAL_CONTENT if received > 0 => Some(response),
            StatusCode::OK => {
                partial
                    .set_len(0)
                    .await
                    .map_err(|error| file_error("truncate", &partial_path, error))?;
                partial
                    .rewind()
                    .await
                    .map_err(|error| file_error("rewind", &partial_path, error))?;
                digest = Sha256::new();
                Some(response)
            }
            // A 416 can mean the partial file is complete; the digest decides.
            StatusCode::RANGE_NOT_SATISFIABLE if received > 0 => None,
            status => {
                return Err(DomainError::InternalError(format!(
                    "{} responded with HTTP {status}",
                    file.url
                )));
            }
        };
        if let Some(response) = rest {
            append(response, &mut partial, &partial_path, &mut digest).await?;
        }
        partial
            .sync_all()
            .await
            .map_err(|error| file_error("sync", &partial_path, error))?;
        drop(partial);

        let actual = hex(&digest.finalize());
        if actual != file.sha256 {
            fs::remove_file(&partial_path)
                .await
                .map_err(|error| file_error("remove", &partial_path, error))?;
            return Err(DomainError::InternalError(format!(
                "{} has SHA-256 {actual}, expected {}",
                file.url, file.sha256
            )));
        }
        fs::rename(&partial_path, path)
            .await
            .map_err(|error| file_error("publish", path, error))
    }
}

async fn digest_file(file: &mut File, digest: &mut Sha256) -> io::Result<u64> {
    let mut buffer = vec![0; IO_BUFFER_BYTES];
    let mut length = 0;
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            return Ok(length);
        }
        digest.update(&buffer[..read]);
        length += read as u64;
    }
}

async fn append(
    mut response: Response,
    file: &mut File,
    path: &Path,
    digest: &mut Sha256,
) -> Result<(), DomainError> {
    let endpoint = response.url().clone();
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, file);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| upstream::body_failure(error, &endpoint))?
    {
        digest.update(&chunk);
        writer
            .write_all(&chunk)
            .await
            .map_err(|error| file_error("write", path, error))?;
    }
    writer
        .flush()
        .await
        .map_err(|error| file_error("write", path, error))
}

fn hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

async fn exists(path: &Path) -> Result<bool, DomainError> {
    fs::try_exists(path)
        .await
        .map_err(|error| file_error("check", path, error))
}

fn file_error(operation: &'static str, path: &Path, source: io::Error) -> DomainError {
    DomainError::FileIo {
        operation,
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;

    use super::*;

    const CONTENT: &[u8] = b"A pinned file, received in two parts.";

    fn leak(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    fn serve_once(response: Vec<u8>) -> (&'static str, Receiver<Option<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let (range_tx, range) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _peer) = listener.accept().unwrap();
            let mut reader = BufReader::new(&stream);
            let mut requested_range = None;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("range:") {
                    requested_range = Some(value.trim().to_string());
                }
                line.clear();
            }
            range_tx.send(requested_range).unwrap();
            stream.write_all(&response).unwrap();
        });
        (leak(url), range)
    }

    fn response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn cache(dir: &Path) -> PinnedFileCache {
        PinnedFileCache::new(
            dir.to_path_buf(),
            Arc::new(HttpClientPool::new("TauriTavern/test")),
        )
    }

    fn entries(dir: &Path) -> Vec<OsString> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    }

    #[tokio::test]
    async fn an_unfinished_download_survives_the_sweep_and_finishes() {
        let (head, rest) = CONTENT.split_at(10);
        for answer in [
            response("206 Partial Content", rest),
            response("200 OK", CONTENT),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (url, range) = serve_once(answer);
            let file = PinnedFile {
                url,
                sha256: leak(hex(&Sha256::digest(CONTENT))),
            };
            std::fs::write(temp.path().join(file.partial_name()), head).unwrap();
            std::fs::create_dir_all(temp.path().join("older-layout").join("blobs")).unwrap();
            std::fs::write(temp.path().join("stale"), b"stale").unwrap();
            let cache = cache(temp.path());

            cache.keep_only(&[file]).await.unwrap();
            let path = cache.get(file).await.unwrap();

            assert_eq!(range.recv().unwrap().as_deref(), Some("bytes=10-"));
            assert_eq!(std::fs::read(&path).unwrap(), CONTENT);
            assert_eq!(entries(temp.path()), [OsString::from(file.sha256)]);
        }
    }

    #[tokio::test]
    async fn a_file_with_another_sha256_is_never_published() {
        let temp = tempfile::tempdir().unwrap();
        let (url, _range) = serve_once(response("200 OK", b"Another file."));
        let file = PinnedFile {
            url,
            sha256: leak(hex(&Sha256::digest(CONTENT))),
        };

        let error = cache(temp.path()).get(file).await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&format!("expected {}", file.sha256)),
            "{error}"
        );
        assert!(entries(temp.path()).is_empty());
    }
}
