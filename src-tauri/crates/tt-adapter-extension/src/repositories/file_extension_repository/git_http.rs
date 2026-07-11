use std::any::Any;
use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gix_transport::client::blocking_io::http::{
    Error as HttpError, GetResponse, Http, PostBodyDataKind, PostResponse,
};
use reqwest::blocking::{Client, ClientBuilder, Request, Response};
use reqwest::header::{
    AUTHORIZATION, HeaderMap, HeaderName, HeaderValue, PROXY_AUTHORIZATION, USER_AGENT,
};
use reqwest::redirect::Policy;
use reqwest::{Method, StatusCode, Url};

const MAX_INITIAL_REDIRECTS: usize = 5;
const UNBOUNDED_POST_ERROR: &str =
    "Unbounded Git HTTP request bodies are outside the supported transport contract";

type SharedSession = Rc<RefCell<Session>>;
type SharedExchange = Rc<RefCell<Exchange>>;

pub(super) struct GitHttp {
    session: SharedSession,
}

pub(super) struct GitHeaders {
    exchange: SharedExchange,
    cursor: Option<Cursor<Vec<u8>>>,
}

pub(super) struct GitResponseBody {
    exchange: SharedExchange,
    reader: Option<BufReader<SafeResponseReader>>,
}

pub(super) struct GitPostBody {
    exchange: SharedExchange,
}

struct Session {
    client: Client,
    redirected_base_url: Option<String>,
    follow_initial_redirect: bool,
    redirect_gate: Arc<AtomicBool>,
}

struct Exchange {
    session: SharedSession,
    state: ExchangeState,
}

enum ExchangeState {
    Pending(PendingRequest),
    Sending,
    Ready {
        headers: Option<Vec<u8>>,
        response: Option<Response>,
    },
    Failed(IoFailure),
}

struct PendingRequest {
    request: Request,
    original_url: String,
    base_url: String,
    upload: Option<Vec<u8>>,
    writer_open: bool,
    follow_redirects: bool,
}

#[derive(Clone)]
struct IoFailure {
    kind: io::ErrorKind,
    message: String,
}

struct SafeResponseReader(Response);

impl GitHttp {
    pub(super) fn new(builder: ClientBuilder) -> Result<Self, HttpError> {
        let redirect_gate = Arc::new(AtomicBool::new(false));
        let client = builder
            .redirect(redirect_policy(Arc::clone(&redirect_gate)))
            .build()
            .map_err(|error| HttpError::InitHttpClient {
                source: Box::new(error.without_url()),
            })?;

        Ok(Self {
            session: Rc::new(RefCell::new(Session {
                client,
                redirected_base_url: None,
                follow_initial_redirect: true,
                redirect_gate,
            })),
        })
    }

    /// Create another serial Git connection for the same operation without
    /// rebuilding the blocking HTTP client.
    pub(super) fn new_session(&self) -> Self {
        let session = self.session.borrow();
        Self {
            session: Rc::new(RefCell::new(Session {
                client: session.client.clone(),
                redirected_base_url: None,
                follow_initial_redirect: true,
                redirect_gate: Arc::clone(&session.redirect_gate),
            })),
        }
    }

    fn exchange(
        &mut self,
        method: Method,
        url: &str,
        base_url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
        upload: Option<Vec<u8>>,
    ) -> Result<SharedExchange, HttpError> {
        let headers = parse_headers(headers)?;
        let original_url = parse_request_url(url)?;
        parse_base_url(base_url)?;
        let tail = url
            .strip_prefix(base_url)
            .ok_or_else(|| HttpError::Detail {
                description: "Git HTTP request URL does not start with its base URL".to_string(),
            })?;

        let mut session = self.session.borrow_mut();
        let effective_url = match session.redirected_base_url.as_deref() {
            Some(redirected_base) => format!("{redirected_base}{tail}"),
            None => original_url.as_str().to_string(),
        };
        let effective_url = Url::parse(&effective_url).map_err(|_error| HttpError::Detail {
            description: "Git HTTP effective URL is invalid".to_string(),
        })?;
        validate_anonymous_http_url(&effective_url)?;

        let follow_redirects = session.follow_initial_redirect && method == Method::GET;
        session.follow_initial_redirect = false;

        let mut request = Request::new(method, effective_url);
        *request.headers_mut() = headers;
        drop(session);

        Ok(Rc::new(RefCell::new(Exchange {
            session: Rc::clone(&self.session),
            state: ExchangeState::Pending(PendingRequest {
                request,
                original_url: url.to_string(),
                base_url: base_url.to_string(),
                writer_open: upload.is_some(),
                upload,
                follow_redirects,
            }),
        })))
    }
}

impl Http for GitHttp {
    type Headers = GitHeaders;
    type ResponseBody = GitResponseBody;
    type PostBody = GitPostBody;

    fn get(
        &mut self,
        url: &str,
        base_url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<GetResponse<Self::Headers, Self::ResponseBody>, HttpError> {
        let exchange = self.exchange(Method::GET, url, base_url, headers, None)?;
        Ok(GetResponse {
            headers: GitHeaders::new(Rc::clone(&exchange)),
            body: GitResponseBody::new(exchange),
        })
    }

    fn post(
        &mut self,
        url: &str,
        base_url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
        body: PostBodyDataKind,
    ) -> Result<PostResponse<Self::Headers, Self::ResponseBody, Self::PostBody>, HttpError> {
        if body == PostBodyDataKind::Unbounded {
            return Err(HttpError::Detail {
                description: UNBOUNDED_POST_ERROR.to_string(),
            });
        }

        let exchange = self.exchange(Method::POST, url, base_url, headers, Some(Vec::new()))?;
        Ok(PostResponse {
            post_body: GitPostBody {
                exchange: Rc::clone(&exchange),
            },
            headers: GitHeaders::new(Rc::clone(&exchange)),
            body: GitResponseBody::new(exchange),
        })
    }

    fn configure(
        &mut self,
        _config: &dyn Any,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        // TauriTavern settings are the sole network policy source.
        Ok(())
    }
}

impl GitHeaders {
    fn new(exchange: SharedExchange) -> Self {
        Self {
            exchange,
            cursor: None,
        }
    }

    fn ensure_cursor(&mut self) -> io::Result<&mut Cursor<Vec<u8>>> {
        if self.cursor.is_none() {
            let headers = self.exchange.borrow_mut().take_headers()?;
            self.cursor = Some(Cursor::new(headers));
        }
        Ok(self.cursor.as_mut().expect("headers were initialized"))
    }
}

impl Read for GitHeaders {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.ensure_cursor()?.read(buffer)
    }
}

impl BufRead for GitHeaders {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.ensure_cursor()?.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.cursor
            .as_mut()
            .expect("fill_buf must be called before consume")
            .consume(amount);
    }
}

impl GitResponseBody {
    fn new(exchange: SharedExchange) -> Self {
        Self {
            exchange,
            reader: None,
        }
    }

    fn ensure_reader(&mut self) -> io::Result<&mut BufReader<SafeResponseReader>> {
        if self.reader.is_none() {
            let response = self.exchange.borrow_mut().take_response()?;
            self.reader = Some(BufReader::new(SafeResponseReader(response)));
        }
        Ok(self.reader.as_mut().expect("response was initialized"))
    }
}

impl Read for GitResponseBody {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.ensure_reader()?.read(buffer)
    }
}

impl BufRead for GitResponseBody {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.ensure_reader()?.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.reader
            .as_mut()
            .expect("fill_buf must be called before consume")
            .consume(amount);
    }
}

impl Write for GitPostBody {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut exchange = self.exchange.borrow_mut();
        let ExchangeState::Pending(pending) = &mut exchange.state else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Git HTTP request body is no longer writable",
            ));
        };
        let upload = pending.upload.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Git HTTP request has no upload body",
            )
        })?;
        upload.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let exchange = self.exchange.borrow();
        match &exchange.state {
            ExchangeState::Pending(pending) if pending.writer_open => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Git HTTP request body is no longer writable",
            )),
        }
    }
}

impl Drop for GitPostBody {
    fn drop(&mut self) {
        if let ExchangeState::Pending(pending) = &mut self.exchange.borrow_mut().state {
            pending.writer_open = false;
        }
    }
}

impl Exchange {
    fn ensure_ready(&mut self) -> io::Result<()> {
        match &self.state {
            ExchangeState::Ready { .. } => return Ok(()),
            ExchangeState::Failed(failure) => return Err(failure.to_io_error()),
            ExchangeState::Sending => {
                return Err(io::Error::other("Git HTTP request is already being sent"));
            }
            ExchangeState::Pending(_) => {}
        }

        let ExchangeState::Pending(pending) =
            std::mem::replace(&mut self.state, ExchangeState::Sending)
        else {
            unreachable!("pending exchange state was checked above");
        };

        if pending.writer_open {
            let failure = IoFailure::new(
                io::ErrorKind::BrokenPipe,
                "Git HTTP response was read before the request body was closed",
            );
            self.state = ExchangeState::Failed(failure.clone());
            return Err(failure.to_io_error());
        }

        match execute_request(&self.session, pending) {
            Ok((headers, response)) => {
                self.state = ExchangeState::Ready {
                    headers: Some(headers),
                    response: Some(response),
                };
                Ok(())
            }
            Err(failure) => {
                self.state = ExchangeState::Failed(failure.clone());
                Err(failure.to_io_error())
            }
        }
    }

    fn take_headers(&mut self) -> io::Result<Vec<u8>> {
        self.ensure_ready()?;
        let ExchangeState::Ready { headers, .. } = &mut self.state else {
            unreachable!("a ready exchange was ensured");
        };
        headers
            .take()
            .ok_or_else(|| io::Error::other("Git HTTP response headers were already consumed"))
    }

    fn take_response(&mut self) -> io::Result<Response> {
        self.ensure_ready()?;
        let ExchangeState::Ready { response, .. } = &mut self.state else {
            unreachable!("a ready exchange was ensured");
        };
        response
            .take()
            .ok_or_else(|| io::Error::other("Git HTTP response body was already consumed"))
    }
}

impl IoFailure {
    fn new(kind: io::ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn from_reqwest(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::new(io::ErrorKind::TimedOut, "Git HTTP request timed out")
        } else if error.is_connect() {
            Self::new(
                io::ErrorKind::ConnectionAborted,
                "Git HTTP connection failed",
            )
        } else {
            Self::new(io::ErrorKind::Other, "Git HTTP request failed")
        }
    }

    fn to_io_error(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}

impl Read for SafeResponseReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0
            .read(buffer)
            .map_err(|error| io::Error::new(error.kind(), "Git HTTP response body read failed"))
    }
}

fn execute_request(
    session: &SharedSession,
    mut pending: PendingRequest,
) -> Result<(Vec<u8>, Response), IoFailure> {
    if let Some(upload) = pending.upload.take() {
        *pending.request.body_mut() = Some(upload.into());
    }

    let effective_url = pending.request.url().as_str().to_string();
    let (client, redirect_gate) = {
        let session = session.borrow();
        (session.client.clone(), Arc::clone(&session.redirect_gate))
    };

    redirect_gate.store(pending.follow_redirects, Ordering::Relaxed);
    let response = client.execute(pending.request);
    redirect_gate.store(false, Ordering::Relaxed);
    let response = response.map_err(IoFailure::from_reqwest)?;

    let status = response.status();
    if !status.is_success() {
        // This follows the public gix `Http` contract, including for 5xx responses.
        let kind = if status == StatusCode::UNAUTHORIZED {
            io::ErrorKind::PermissionDenied
        } else {
            io::ErrorKind::Other
        };
        return Err(IoFailure::new(
            kind,
            format!("Git HTTP request returned status {}", status.as_u16()),
        ));
    }

    if response.url().as_str() != effective_url {
        let redirected_base = redirected_base_url(
            response.url(),
            pending.base_url.as_str(),
            pending.original_url.as_str(),
        )?;
        session.borrow_mut().redirected_base_url = Some(redirected_base);
    }

    let mut headers = Vec::new();
    for (name, value) in response.headers() {
        headers.extend_from_slice(name.as_str().as_bytes());
        headers.push(b':');
        headers.extend_from_slice(value.as_bytes());
        headers.push(b'\n');
    }

    Ok((headers, response))
}

fn parse_headers(
    headers: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<HeaderMap, HttpError> {
    let mut parsed = HeaderMap::new();
    for line in headers {
        let (name, value) = line
            .as_ref()
            .split_once(':')
            .ok_or_else(|| HttpError::Detail {
                description: "Git HTTP header is missing a colon".to_string(),
            })?;
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_error| HttpError::Detail {
            description: "Git HTTP header name is invalid".to_string(),
        })?;

        if name == USER_AGENT {
            continue;
        }
        if name == AUTHORIZATION || name == PROXY_AUTHORIZATION {
            return Err(HttpError::Detail {
                description: "Authenticated Git HTTP requests are not supported".to_string(),
            });
        }

        let value = HeaderValue::from_bytes(value.trim().as_bytes()).map_err(|_error| {
            HttpError::Detail {
                description: "Git HTTP header value is invalid".to_string(),
            }
        })?;
        parsed.append(name, value);
    }
    Ok(parsed)
}

fn parse_request_url(value: &str) -> Result<Url, HttpError> {
    let url = Url::parse(value).map_err(|_error| HttpError::Detail {
        description: "Git HTTP request URL is invalid".to_string(),
    })?;
    validate_anonymous_http_url(&url)?;
    if url.fragment().is_some() {
        return Err(HttpError::Detail {
            description: "Git HTTP request URL must not contain a fragment".to_string(),
        });
    }
    Ok(url)
}

fn parse_base_url(value: &str) -> Result<Url, HttpError> {
    let url = Url::parse(value).map_err(|_error| HttpError::Detail {
        description: "Git HTTP base URL is invalid".to_string(),
    })?;
    validate_anonymous_http_url(&url)?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(HttpError::Detail {
            description: "Git HTTP base URL must not contain a query or fragment".to_string(),
        });
    }
    Ok(url)
}

fn validate_anonymous_http_url(url: &Url) -> Result<(), HttpError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(HttpError::Detail {
            description: "Only HTTP and HTTPS Git remotes are supported".to_string(),
        });
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(HttpError::Detail {
            description: "Authenticated Git remote URLs are not supported".to_string(),
        });
    }
    Ok(())
}

fn redirect_policy(gate: Arc<AtomicBool>) -> Policy {
    Policy::custom(move |attempt| {
        if !gate.load(Ordering::Relaxed) {
            return attempt.stop();
        }
        if attempt.previous().len() > MAX_INITIAL_REDIRECTS {
            return attempt.error("Git HTTP redirect limit exceeded");
        }
        let Some(original) = attempt.previous().first() else {
            return attempt.error("Git HTTP redirect chain is invalid");
        };
        if shares_authority_or_upgrades_scheme(attempt.url(), original) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

fn shares_authority_or_upgrades_scheme(redirect: &Url, original: &Url) -> bool {
    if !redirect.username().is_empty()
        || redirect.password().is_some()
        || redirect.host_str() != original.host_str()
    {
        return false;
    }
    if redirect.scheme() == original.scheme() {
        return redirect.port_or_known_default() == original.port_or_known_default();
    }
    original.scheme() == "http"
        && redirect.scheme() == "https"
        && (original.port_or_known_default() == redirect.port_or_known_default()
            || matches!(
                (
                    original.port_or_known_default(),
                    redirect.port_or_known_default()
                ),
                (Some(80), Some(443))
            ))
}

fn redirected_base_url(
    redirected_url: &Url,
    base_url: &str,
    original_url: &str,
) -> Result<String, IoFailure> {
    let base = Url::parse(base_url).map_err(|_error| {
        IoFailure::new(io::ErrorKind::Other, "Git HTTP redirect base is invalid")
    })?;
    if !shares_authority_or_upgrades_scheme(redirected_url, &base) {
        return Err(IoFailure::new(
            io::ErrorKind::Other,
            "Git HTTP redirect changed authority or downgraded TLS",
        ));
    }
    let tail = original_url.strip_prefix(base_url).ok_or_else(|| {
        IoFailure::new(
            io::ErrorKind::Other,
            "Git HTTP redirect request path is invalid",
        )
    })?;
    redirected_url
        .as_str()
        .strip_suffix(tail)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            IoFailure::new(
                io::ErrorKind::Other,
                "Git HTTP redirect did not preserve the request path",
            )
        })
}

#[cfg(test)]
mod tests;
