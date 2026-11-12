//! TT extension fields must not leak into formats that copy messages verbatim.

use serde_json::{Map, Value};

use crate::errors::ApplicationError;
use crate::services::chat_completion_service::exchange::ChatCompletionProviderFormat;
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

pub(super) fn project_extension_fields(
    payload: &mut Map<String, Value>,
    source: ChatCompletionSource,
    format: ChatCompletionProviderFormat,
) -> Result<(), ApplicationError> {
    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let native_key = format.native_key();
    let keeps_signatures = reads_signatures(source, format);

    for message in messages.iter_mut().filter_map(Value::as_object_mut) {
        retain_native(message, native_key)?;
        if !keeps_signatures {
            remove_signatures(message);
        }
    }
    Ok(())
}

fn reads_signatures(source: ChatCompletionSource, format: ChatCompletionProviderFormat) -> bool {
    matches!(
        format,
        ChatCompletionProviderFormat::Gemini | ChatCompletionProviderFormat::GeminiInteractions
    ) || source == ChatCompletionSource::OpenRouter
}

fn retain_native(
    message: &mut Map<String, Value>,
    native_key: Option<&str>,
) -> Result<(), ApplicationError> {
    let mut native = match message.remove("native") {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Object(native)) => native,
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "chat_completion.invalid_native: message `native` must be an object keyed by API format"
                    .to_string(),
            ));
        }
    };
    native.retain(|key, _| Some(key.as_str()) == native_key);
    if !native.is_empty() {
        message.insert("native".to_string(), Value::Object(native));
    }
    Ok(())
}

fn remove_signatures(message: &mut Map<String, Value>) {
    message.remove("signature");
    if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for call in calls.iter_mut().filter_map(Value::as_object_mut) {
            call.remove("signature");
        }
    }
}
