//! An empty body with `x-tauritavern-body-stream: <id> <length>` transfers the stream to
//! `HostResourceBodyStream.kt`, which releases it on close or collection.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass};
use jni::sys::{jint, jlong};
use tauri::http::HeaderValue;
use tt_ports::host_resource::{HostResourceStoreError, HostResourceStream};

const BODY_STREAM_HEADER: &str = "x-tauritavern-body-stream";
const SKIP_CHUNK_BYTES: usize = 64 * 1024;

type SharedStream = Arc<Mutex<HostResourceStream>>;

static STREAMS: LazyLock<Mutex<HashMap<jlong, SharedStream>>> = LazyLock::new(Default::default);
static NEXT_STREAM_ID: AtomicI64 = AtomicI64::new(1);

pub(super) fn hand_over(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    stream: HostResourceStream,
) -> Cow<'static, [u8]> {
    let length = stream.length();
    let id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
    streams().insert(id, Arc::new(Mutex::new(stream)));
    response.headers_mut().insert(
        BODY_STREAM_HEADER,
        HeaderValue::from_str(&format!("{id} {length}")).expect("numeric header value"),
    );
    Cow::Borrowed(&[])
}

fn streams() -> std::sync::MutexGuard<'static, HashMap<jlong, SharedStream>> {
    STREAMS.lock().expect("host resource stream registry lock")
}

fn with_stream<T>(
    id: jlong,
    action: impl FnOnce(&mut HostResourceStream) -> Result<T, HostResourceStoreError>,
) -> Result<T, String> {
    let stream = streams()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("Host resource stream {id} is closed"))?;
    let mut stream = stream.lock().expect("host resource stream lock");
    action(&mut stream).map_err(|error| match error {
        HostResourceStoreError::NotFound(message)
        | HostResourceStoreError::Forbidden(message)
        | HostResourceStoreError::Internal(message) => message,
    })
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
            let mut chunk = vec![0u8; length];
            let count = with_stream(id, |stream| stream.read(&mut chunk))?;
            // SAFETY: u8 and i8 share size and alignment, and every bit pattern is valid for both.
            let bytes = unsafe { std::slice::from_raw_parts(chunk.as_ptr().cast::<i8>(), count) };
            env.set_byte_array_region(&buffer, offset, bytes)
                .map_err(|error| error.to_string())?;
            Ok(count as jint)
        });
    result.unwrap_or_else(|message| {
        throw_io_error(&mut env, message);
        -1
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_tauritavern_client_HostResourceBodyStream_skip(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    count: jlong,
) -> jlong {
    let result = u64::try_from(count)
        .map_err(|_| format!("Invalid host resource skip count: {count}"))
        .and_then(|wanted| {
            with_stream(id, |stream| {
                let mut chunk = vec![0u8; SKIP_CHUNK_BYTES];
                let mut skipped = 0;
                while skipped < wanted {
                    let limit = (wanted - skipped).min(SKIP_CHUNK_BYTES as u64) as usize;
                    match stream.read(&mut chunk[..limit])? {
                        0 => break,
                        read => skipped += read as u64,
                    }
                }
                Ok(skipped as jlong)
            })
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
