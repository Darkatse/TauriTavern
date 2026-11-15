use std::io::{Cursor, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use http::{Method, Request};
use tt_contracts::range::ByteRange;
use tt_ports::host_resource::{
    HostResourceBody, HostResourceBodyReader, HostResourceSourceMetadata,
    HostResourceSourceRevision, HostResourceStoreError, OpenedHostResource,
};

use super::{HostResourceResponse, HostResourceResponseBody};

struct BytesBody {
    bytes: Vec<u8>,
    reads: Arc<AtomicUsize>,
}

impl HostResourceBody for BytesBody {
    fn into_reader(
        self: Box<Self>,
        range: Option<ByteRange>,
    ) -> Result<Box<dyn HostResourceBodyReader>, HostResourceStoreError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let bytes = match range {
            Some(range) => self.bytes[range.start() as usize..=range.end() as usize].to_vec(),
            None => self.bytes,
        };
        Ok(Box::new(BytesReader(Cursor::new(bytes))))
    }
}

struct BytesReader(Cursor<Vec<u8>>);

impl HostResourceBodyReader for BytesReader {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, HostResourceStoreError> {
        Ok(self.0.read(buffer).expect("in-memory read"))
    }
}

pub(super) fn opened(
    bytes: &[u8],
    content_type: &str,
    reads: Arc<AtomicUsize>,
) -> OpenedHostResource {
    OpenedHostResource::new(
        HostResourceSourceMetadata {
            content_type: content_type.to_string(),
            content_length: bytes.len() as u64,
            last_modified: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            revision: HostResourceSourceRevision::new(b"test-revision".to_vec()),
        },
        Box::new(BytesBody {
            bytes: bytes.to_vec(),
            reads,
        }),
    )
}

pub(super) fn request(method: Method, uri: &str) -> Request<Vec<u8>> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Vec::new())
        .expect("request")
}

pub(super) fn body_bytes(response: &HostResourceResponse) -> &[u8] {
    match response.body() {
        HostResourceResponseBody::Bytes(bytes) => bytes,
        HostResourceResponseBody::Stream(_) => panic!("the body was streamed"),
    }
}
