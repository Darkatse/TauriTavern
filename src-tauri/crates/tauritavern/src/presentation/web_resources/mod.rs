#[cfg(target_os = "android")]
mod android_body_stream;
#[cfg(any(dev, debug_assertions))]
pub mod dev_protocol_endpoint;
pub mod tauri_resource_adapter;
