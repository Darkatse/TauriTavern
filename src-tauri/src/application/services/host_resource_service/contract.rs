#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostResourceMethod {
    Get,
    Head,
    Options,
    Other,
}

impl HostResourceMethod {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "GET" => Self::Get,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostResourceHeader<'a> {
    pub(crate) name: &'a str,
    pub(crate) value: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostResourceHeaders<'a> {
    entries: &'a [HostResourceHeader<'a>],
}

impl<'a> HostResourceHeaders<'a> {
    #[cfg(test)]
    pub(crate) const fn empty() -> Self {
        Self { entries: &[] }
    }

    pub(crate) const fn new(entries: &'a [HostResourceHeader<'a>]) -> Self {
        Self { entries }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&'a [u8]> {
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostResourceRequest<'a> {
    pub(crate) method: HostResourceMethod,
    pub(crate) path: &'a str,
    pub(crate) query: Option<&'a str>,
    pub(crate) headers: HostResourceHeaders<'a>,
}

impl<'a> HostResourceRequest<'a> {
    pub(crate) const fn new(
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
pub(crate) struct HostResourceResponse {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl HostResourceResponse {
    pub(crate) fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    pub(crate) fn no_content(allowed_methods: &'static str) -> Self {
        Self::new(status::NO_CONTENT, Vec::new())
            .with_header(header::ALLOW, allowed_methods)
            .with_header(header::CACHE_CONTROL, "no-store")
    }

    pub(crate) fn plain_text(status: u16, message: &str) -> Self {
        Self::new(status, message.as_bytes().to_vec())
            .with_header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .with_header(header::CACHE_CONTROL, "no-store")
    }

    pub(crate) fn method_not_allowed(allowed_methods: &'static str) -> Self {
        Self::plain_text(status::METHOD_NOT_ALLOWED, "Method not allowed")
            .with_header(header::ALLOW, allowed_methods)
    }

    pub(crate) fn bytes(status: u16, bytes: Vec<u8>, content_type: &str) -> Self {
        Self::new(status, bytes)
            .with_header(header::CONTENT_TYPE, content_type)
            .with_header(header::CACHE_CONTROL, "no-store")
    }

    pub(crate) fn with_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }
}

pub(crate) mod header {
    pub(crate) const ACCEPT_RANGES: &str = "accept-ranges";
    pub(crate) const ALLOW: &str = "allow";
    pub(crate) const CACHE_CONTROL: &str = "cache-control";
    pub(crate) const CONTENT_LENGTH: &str = "content-length";
    pub(crate) const CONTENT_RANGE: &str = "content-range";
    pub(crate) const CONTENT_TYPE: &str = "content-type";
    pub(crate) const RANGE: &str = "range";
    pub(crate) const TAURITAVERN_TRACE_ID: &str = "x-tauritavern-trace-id";
}

pub(crate) mod status {
    pub(crate) const OK: u16 = 200;
    pub(crate) const NO_CONTENT: u16 = 204;
    pub(crate) const PARTIAL_CONTENT: u16 = 206;
    pub(crate) const BAD_REQUEST: u16 = 400;
    pub(crate) const FORBIDDEN: u16 = 403;
    pub(crate) const NOT_FOUND: u16 = 404;
    pub(crate) const METHOD_NOT_ALLOWED: u16 = 405;
    pub(crate) const RANGE_NOT_SATISFIABLE: u16 = 416;
    pub(crate) const PAYLOAD_TOO_LARGE: u16 = 413;
    pub(crate) const INTERNAL_SERVER_ERROR: u16 = 500;
}
