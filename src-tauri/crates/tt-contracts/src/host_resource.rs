#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostResourceMethod {
    Get,
    Head,
    Options,
    Other,
}

impl HostResourceMethod {
    pub fn from_str(value: &str) -> Self {
        match value {
            "GET" => Self::Get,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostResourceHeader<'a> {
    pub name: &'a str,
    pub value: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostResourceHeaders<'a> {
    entries: &'a [HostResourceHeader<'a>],
}

impl<'a> HostResourceHeaders<'a> {
    pub const fn empty() -> Self {
        Self { entries: &[] }
    }

    pub const fn new(entries: &'a [HostResourceHeader<'a>]) -> Self {
        Self { entries }
    }

    pub fn get(&self, name: &str) -> Option<&'a [u8]> {
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostResourceRequest<'a> {
    pub method: HostResourceMethod,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub headers: HostResourceHeaders<'a>,
}

impl<'a> HostResourceRequest<'a> {
    pub const fn new(
        method: HostResourceMethod,
        path: &'a str,
        query: Option<&'a str>,
        headers: HostResourceHeaders<'a>,
    ) -> Self {
        Self {
            method,
            path,
            query,
            headers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostResourceResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HostResourceResponse {
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    pub fn no_content(allowed_methods: &'static str) -> Self {
        Self::new(status::NO_CONTENT, Vec::new())
            .with_header(header::ALLOW, allowed_methods)
            .with_header(header::CACHE_CONTROL, "no-store")
    }

    pub fn plain_text(status: u16, message: &str) -> Self {
        Self::new(status, message.as_bytes().to_vec())
            .with_header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .with_header(header::CACHE_CONTROL, "no-store")
    }

    pub fn method_not_allowed(allowed_methods: &'static str) -> Self {
        Self::plain_text(status::METHOD_NOT_ALLOWED, "Method not allowed")
            .with_header(header::ALLOW, allowed_methods)
    }

    pub fn bytes(status: u16, bytes: Vec<u8>, content_type: &str) -> Self {
        Self::new(status, bytes)
            .with_header(header::CONTENT_TYPE, content_type)
            .with_header(header::CACHE_CONTROL, "no-store")
    }

    pub fn with_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }
}

pub mod header {
    pub const ACCEPT_RANGES: &str = "accept-ranges";
    pub const ALLOW: &str = "allow";
    pub const CACHE_CONTROL: &str = "cache-control";
    pub const CONTENT_LENGTH: &str = "content-length";
    pub const CONTENT_RANGE: &str = "content-range";
    pub const CONTENT_TYPE: &str = "content-type";
    pub const RANGE: &str = "range";
    pub const TAURITAVERN_TRACE_ID: &str = "x-tauritavern-trace-id";
}

pub mod status {
    pub const OK: u16 = 200;
    pub const NO_CONTENT: u16 = 204;
    pub const PARTIAL_CONTENT: u16 = 206;
    pub const BAD_REQUEST: u16 = 400;
    pub const FORBIDDEN: u16 = 403;
    pub const NOT_FOUND: u16 = 404;
    pub const METHOD_NOT_ALLOWED: u16 = 405;
    pub const RANGE_NOT_SATISFIABLE: u16 = 416;
    pub const PAYLOAD_TOO_LARGE: u16 = 413;
    pub const INTERNAL_SERVER_ERROR: u16 = 500;
}
