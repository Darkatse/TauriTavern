//! An empty body with `x-tauritavern-body-stream: <id> <length>` transfers the stream to
//! `HostResourceBodyStream.kt`, which releases it on close or collection.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass};
use jni::sys::{jint, jlong};
use tauri::http::HeaderValue;
use tt_ports::host_resource::{HostResourceStoreError, HostResourceStream};

const BODY_STREAM_HEADER: &str = "x-tauritavern-body-stream";
// The WebView reads 4 KiB per call.
const READ_BUFFER_BYTES: u64 = 64 * 1024;

type SharedStream = Arc<Mutex<BufReader<StreamReader>>>;

static STREAMS: LazyLock<Mutex<HashMap<jlong, SharedStream>>> = LazyLock::new(Default::default);
static NEXT_STREAM_ID: AtomicI64 = AtomicI64::new(1);

struct StreamReader(HostResourceStream);

impl Read for StreamReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer).map_err(|error| {
            io::Error::other(match error {
                HostResourceStoreError::NotFound(message)
                | HostResourceStoreError::Forbidden(message)
                | HostResourceStoreError::Internal(message) => message,
            })
        })
    }
}

pub(super) fn hand_over(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    stream: HostResourceStream,
) -> Cow<'static, [u8]> {
    let length = stream.length();
    let id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
    let reader =
        BufReader::with_capacity(length.min(READ_BUFFER_BYTES) as usize, StreamReader(stream));
    streams().insert(id, Arc::new(Mutex::new(reader)));
    response.headers_mut().insert(
        BODY_STREAM_HEADER,
        HeaderValue::from_str(&format!("{id} {length}")).expect("numeric header value"),
    );
    Cow::Borrowed(&[])
}

fn streams() -> std::sync::MutexGuard<'static, HashMap<jlong, SharedStream>> {
    STREAMS.lock().expect("host resource stream registry lock")
}

fn stream(id: jlong) -> Result<SharedStream, String> {
    streams()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("Host resource stream {id} is closed"))
}

fn throw_io_error(env: &mut JNIEnv, message: String) {
    if matches!(env.exception_check(), Ok(true)) {
        return;
    }
    if let Err(error) = env.throw_new("java/io/IOException", message) {
        tracing::error!("Failed to raise a host resource stream error: {error}");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_tauritavern_client_HostResourceBodyStream_read(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    buffer: JByteArray,
    offset: jint,
    length: jint,
) -> jint {
    let result = usize::try_from(length)
        .map_err(|_| format!("Invalid host resource read length: {length}"))
        .and_then(|length| {
            let stream = stream(id)?;
            let mut stream = stream.lock().expect("host resource stream lock");
            let buffered = stream.fill_buf().map_err(|error| error.to_string())?;
            let count = buffered.len().min(length);
            // SAFETY: u8 and i8 share size and alignment, and every bit pattern is valid for both.
            let bytes =
                unsafe { std::slice::from_raw_parts(buffered.as_ptr().cast::<i8>(), count) };
            env.set_byte_array_region(&buffer, offset, bytes)
                .map_err(|error| error.to_string())?;
            stream.consume(count);
            Ok(count as jint)
        });
    result.unwrap_or_else(|message| {
        throw_io_error(&mut env, message);
        -1
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_tauritavern_client_HostResourceBodyStream_close(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) {
    streams().remove(&id);
}
