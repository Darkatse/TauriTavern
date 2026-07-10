use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

use super::shared::parse_data_url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InputPart {
    Text(String),
    Image(MediaPart),
    Audio(MediaPart),
    Video(MediaPart),
    Unknown { ty: Option<String>, value: Value },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MediaPart {
    pub(super) source: MediaSource,
    pub(super) detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MediaSource {
    DataUrl { mime_type: String, data: String },
    Url(String),
    FileId(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioInputFormatSet {
    OpenAi,
    OpenRouter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioRewriteFallbackPolicy {
    Reject,
    Preserve,
}

impl MediaKind {
    fn field(self) -> &'static str {
        match self {
            Self::Image => "image_url",
            Self::Audio => "audio_url",
            Self::Video => "video_url",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }

    fn part(self, media: MediaPart) -> InputPart {
        match self {
            Self::Image => InputPart::Image(media),
            Self::Audio => InputPart::Audio(media),
            Self::Video => InputPart::Video(media),
        }
    }
}

pub(super) fn parse_openai_chat_content(
    content: Option<&Value>,
) -> Result<Vec<InputPart>, ApplicationError> {
    let Some(content) = content else {
        return Ok(Vec::new());
    };

    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![InputPart::Text(text.clone())]),
        Value::Array(parts) => parts.iter().map(parse_content_part).collect(),
        Value::Object(object) => Ok(vec![parse_object_content_part(object)?]),
        other => Ok(vec![InputPart::Text(other.to_string())]),
    }
}

pub(super) fn reject_media_for_text_only_provider(
    provider_name: &str,
    messages: Option<&Value>,
) -> Result<(), ApplicationError> {
    let Some(messages) = messages else {
        return Ok(());
    };

    let Some(entries) = messages.as_array() else {
        return Ok(());
    };

    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        reject_media_for_text_only_content(provider_name, message.get("content"))?;
    }

    Ok(())
}

pub(super) fn reject_media_for_text_only_content(
    provider_name: &str,
    content: Option<&Value>,
) -> Result<(), ApplicationError> {
    if let Some(kind) = first_media_kind_in_content(content) {
        return Err(ApplicationError::ValidationError(format!(
            "{provider_name} cannot preserve {kind} input in this text-only provider format. Use a multimodal provider format or disable {kind} inlining."
        )));
    }

    Ok(())
}

pub(super) fn reject_video_for_provider(
    provider_name: &str,
    messages: Option<&Value>,
) -> Result<(), ApplicationError> {
    let Some(messages) = messages else {
        return Ok(());
    };

    let Some(entries) = messages.as_array() else {
        return Ok(());
    };

    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        if contains_media_kind(message.get("content"), "video") {
            return Err(ApplicationError::ValidationError(format!(
                "{provider_name} cannot preserve video input in this provider format. Use a provider that supports video input or disable video inlining."
            )));
        }
    }

    Ok(())
}

pub(super) fn rewrite_audio_url_parts_to_input_audio(
    provider_name: &str,
    messages: Option<&mut Value>,
    formats: AudioInputFormatSet,
    fallback_policy: AudioRewriteFallbackPolicy,
) -> Result<(), ApplicationError> {
    let Some(messages) = messages else {
        return Ok(());
    };

    let Some(entries) = messages.as_array_mut() else {
        return Ok(());
    };

    for entry in entries {
        let Some(message) = entry.as_object_mut() else {
            continue;
        };

        rewrite_audio_url_content_parts(
            provider_name,
            message.get_mut("content"),
            formats,
            fallback_policy,
        )?;
    }

    Ok(())
}

/// Lossy text projection. Callers must reject unsupported non-text parts first.
pub(super) fn to_lossy_text(parts: &[InputPart]) -> String {
    let mut text = String::new();
    for part in parts {
        match part {
            InputPart::Text(fragment) => text.push_str(fragment),
            InputPart::Unknown { value, .. } => {
                if let Some(fragment) = object_text(value) {
                    text.push_str(fragment);
                }
            }
            InputPart::Image(_) | InputPart::Audio(_) | InputPart::Video(_) => {}
        }
    }
    text
}

pub(super) fn openai_chat_content_to_lossy_text(content: Option<&Value>) -> String {
    // Preserve legacy projection for callers that cannot return validation errors yet.
    parse_openai_chat_content(content)
        .map(|parts| to_lossy_text(&parts))
        .unwrap_or_else(|_| legacy_lossy_text(content))
}

fn parse_content_part(part: &Value) -> Result<InputPart, ApplicationError> {
    match part {
        Value::String(fragment) => Ok(InputPart::Text(fragment.clone())),
        Value::Object(object) => parse_object_content_part(object),
        other => Ok(InputPart::Unknown {
            ty: None,
            value: other.clone(),
        }),
    }
}

fn parse_object_content_part(object: &Map<String, Value>) -> Result<InputPart, ApplicationError> {
    let ty = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match ty {
        Some("text") | Some("input_text") => Ok(InputPart::Text(
            object
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        )),
        Some("image_url") => parse_media_url_part(object, MediaKind::Image),
        Some("audio_url") => parse_media_url_part(object, MediaKind::Audio),
        Some("video_url") => parse_media_url_part(object, MediaKind::Video),
        Some("input_image") => parse_input_image_part(object),
        Some(value) => Ok(InputPart::Unknown {
            ty: Some(value.to_string()),
            value: Value::Object(object.clone()),
        }),
        None => Ok((!has_provider_native_part_key(object))
            .then(|| object_text_value(object))
            .flatten()
            .map(InputPart::Text)
            .unwrap_or_else(|| InputPart::Unknown {
                ty: None,
                value: Value::Object(object.clone()),
            })),
    }
}

fn parse_media_url_part(
    object: &Map<String, Value>,
    kind: MediaKind,
) -> Result<InputPart, ApplicationError> {
    let field = kind.field();
    let entry = object.get(field).and_then(Value::as_object);
    let url = entry.and_then(|entry| non_empty_string(entry.get("url")));
    let Some(url) = url else {
        return Err(ApplicationError::ValidationError(format!(
            "{} content part is missing url",
            kind.label()
        )));
    };

    let detail = entry
        .and_then(|entry| entry.get("detail"))
        .map(optional_string)
        .transpose()?
        .flatten();

    Ok(kind.part(MediaPart {
        source: parse_media_source(kind.label(), &url)?,
        detail,
    }))
}

fn parse_input_image_part(object: &Map<String, Value>) -> Result<InputPart, ApplicationError> {
    let detail = object
        .get("detail")
        .map(optional_string)
        .transpose()?
        .flatten();

    if let Some(url) = non_empty_string(object.get("image_url")) {
        return Ok(InputPart::Image(MediaPart {
            source: parse_media_source("input_image", &url)?,
            detail,
        }));
    }

    if let Some(file_id) = non_empty_string(object.get("file_id")) {
        return Ok(InputPart::Image(MediaPart {
            source: MediaSource::FileId(file_id),
            detail,
        }));
    }

    Err(ApplicationError::ValidationError(
        "input_image content part is missing image_url or file_id".to_string(),
    ))
}

fn parse_media_source(label: &str, url: &str) -> Result<MediaSource, ApplicationError> {
    if url.starts_with("data:") {
        let Some((mime_type, data)) = parse_data_url(url) else {
            return Err(ApplicationError::ValidationError(format!(
                "{label} content part has invalid data URL"
            )));
        };

        return Ok(MediaSource::DataUrl { mime_type, data });
    }

    Ok(MediaSource::Url(url.to_string()))
}

fn first_media_kind_in_content(content: Option<&Value>) -> Option<&'static str> {
    match content? {
        Value::Array(parts) => parts.iter().find_map(media_kind_in_value),
        Value::Object(object) => media_kind_in_object(object),
        _ => None,
    }
}

pub(super) fn media_kind_in_value(value: &Value) -> Option<&'static str> {
    value.as_object().and_then(media_kind_in_object)
}

fn contains_media_kind(content: Option<&Value>, rejected_kind: &str) -> bool {
    match content {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(media_kind_in_value)
            .any(|kind| kind == rejected_kind),
        Some(Value::Object(object)) => media_kind_in_object(object) == Some(rejected_kind),
        _ => false,
    }
}

fn rewrite_audio_url_content_parts(
    provider_name: &str,
    content: Option<&mut Value>,
    formats: AudioInputFormatSet,
    fallback_policy: AudioRewriteFallbackPolicy,
) -> Result<(), ApplicationError> {
    match content {
        Some(Value::Array(parts)) => {
            for part in parts {
                rewrite_audio_url_part(provider_name, part, formats, fallback_policy)?;
            }
        }
        Some(part @ Value::Object(_))
            if fallback_policy == AudioRewriteFallbackPolicy::Reject
                && media_kind_in_value(part) == Some("audio") =>
        {
            return Err(ApplicationError::ValidationError(format!(
                "{provider_name} audio content parts must be inside a content array."
            )));
        }
        _ => {}
    }

    Ok(())
}

fn rewrite_audio_url_part(
    provider_name: &str,
    part: &mut Value,
    formats: AudioInputFormatSet,
    fallback_policy: AudioRewriteFallbackPolicy,
) -> Result<(), ApplicationError> {
    let Some(object) = part.as_object_mut() else {
        return Ok(());
    };

    if object.get("type").and_then(Value::as_str).map(str::trim) != Some("audio_url") {
        return Ok(());
    }

    let Some(url) = object
        .get("audio_url")
        .and_then(Value::as_object)
        .and_then(|entry| non_empty_string(entry.get("url")))
    else {
        if fallback_policy == AudioRewriteFallbackPolicy::Preserve {
            return Ok(());
        }

        return Err(ApplicationError::ValidationError(
            "audio content part is missing url".to_string(),
        ));
    };

    let Some((mime_type, data)) = parse_data_url(&url) else {
        if fallback_policy == AudioRewriteFallbackPolicy::Preserve {
            return Ok(());
        }

        let message = if url.starts_with("data:") {
            "audio content part has invalid data URL".to_string()
        } else {
            format!(
                "{provider_name} audio content part must be a base64 data URL; remote audio URLs are not supported."
            )
        };
        return Err(ApplicationError::ValidationError(message));
    };

    let Some(format) = audio_input_format(formats, &mime_type) else {
        if fallback_policy == AudioRewriteFallbackPolicy::Preserve {
            return Ok(());
        }

        return Err(ApplicationError::ValidationError(format!(
            "{provider_name} audio content part uses unsupported audio data URL MIME type: {mime_type}. Supported by this app mapping: {}.",
            supported_audio_formats(formats)
        )));
    };

    object.remove("audio_url");
    object.insert("type".to_string(), Value::String("input_audio".to_string()));
    object.insert(
        "input_audio".to_string(),
        json!({
            "format": format,
            "data": data,
        }),
    );

    Ok(())
}

fn audio_input_format(formats: AudioInputFormatSet, mime_type: &str) -> Option<&'static str> {
    let normalized = mime_type.trim().to_ascii_lowercase();

    match formats {
        AudioInputFormatSet::OpenAi => match normalized.as_str() {
            "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => Some("wav"),
            "audio/mpeg" | "audio/mp3" => Some("mp3"),
            _ => None,
        },
        AudioInputFormatSet::OpenRouter => match normalized.as_str() {
            "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => Some("wav"),
            "audio/mpeg" | "audio/mp3" => Some("mp3"),
            "audio/aiff" | "audio/x-aiff" => Some("aiff"),
            "audio/aac" => Some("aac"),
            "audio/ogg" | "application/ogg" => Some("ogg"),
            "audio/flac" | "audio/x-flac" => Some("flac"),
            "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Some("m4a"),
            _ => None,
        },
    }
}

fn supported_audio_formats(formats: AudioInputFormatSet) -> &'static str {
    match formats {
        AudioInputFormatSet::OpenAi => "wav, mp3",
        AudioInputFormatSet::OpenRouter => "wav, mp3, aiff, aac, ogg, flac, m4a",
    }
}

fn media_kind_in_object(object: &Map<String, Value>) -> Option<&'static str> {
    if let Some(kind) = native_media_kind(
        object
            .get("inlineData")
            .or_else(|| object.get("inline_data")),
    ) {
        return Some(kind);
    }
    if object.get("inlineData").is_some() || object.get("inline_data").is_some() {
        return Some("media");
    }

    if object.get("fileData").is_some() || object.get("file_data").is_some() {
        return Some("file");
    }

    let ty = object.get("type").and_then(Value::as_str)?.trim();
    match ty {
        "image_url" | "input_image" | "image" => Some("image"),
        "audio_url" | "input_audio" | "audio" => Some("audio"),
        "video_url" | "input_video" | "video" => Some("video"),
        "input_file" | "file" => Some("file"),
        _ => None,
    }
}

fn native_media_kind(value: Option<&Value>) -> Option<&'static str> {
    let mime_type = value
        .and_then(Value::as_object)
        .and_then(|object| object.get("mimeType").or_else(|| object.get("mime_type")))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase())?;

    if mime_type.starts_with("image/") {
        Some("image")
    } else if mime_type.starts_with("audio/") {
        Some("audio")
    } else if mime_type.starts_with("video/") {
        Some("video")
    } else {
        None
    }
}

fn has_provider_native_part_key(object: &Map<String, Value>) -> bool {
    [
        "inlineData",
        "inline_data",
        "fileData",
        "file_data",
        "functionCall",
        "function_call",
        "functionResponse",
        "function_response",
    ]
    .iter()
    .any(|key| object.contains_key(*key))
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_string(value: &Value) -> Result<Option<String>, ApplicationError> {
    if value.is_null() {
        return Ok(None);
    }

    let Some(value) = value.as_str() else {
        return Err(ApplicationError::ValidationError(
            "content part detail must be a string".to_string(),
        ));
    };

    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn object_text(value: &Value) -> Option<&str> {
    value.as_object().and_then(|object| {
        object
            .get("text")
            .or_else(|| object.get("content"))
            .and_then(Value::as_str)
    })
}

fn object_text_value(object: &Map<String, Value>) -> Option<String> {
    object
        .get("text")
        .or_else(|| object.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn legacy_lossy_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };

    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    Value::String(fragment) => text.push_str(fragment),
                    Value::Object(object) => {
                        if let Some(fragment) = object.get("text").and_then(Value::as_str) {
                            text.push_str(fragment);
                        } else if let Some(fragment) = object.get("content").and_then(Value::as_str)
                        {
                            text.push_str(fragment);
                        }
                    }
                    _ => {}
                }
            }
            text
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        InputPart, MediaPart, MediaSource, parse_openai_chat_content,
        reject_media_for_text_only_provider, to_lossy_text,
    };

    #[test]
    fn parses_plain_text() {
        let parts = parse_openai_chat_content(Some(&json!("hello"))).expect("text should parse");

        assert_eq!(parts, vec![InputPart::Text("hello".to_string())]);
        assert!(!parts.iter().any(is_media_part));
        assert_eq!(to_lossy_text(&parts), "hello");
    }

    #[test]
    fn parses_legacy_image_audio_video_parts() {
        let parts = parse_openai_chat_content(Some(&json!([
            { "type": "text", "text": "describe" },
            {
                "type": "image_url",
                "image_url": {
                    "url": "data:image/png;base64,AAAA",
                    "detail": "high"
                }
            },
            {
                "type": "audio_url",
                "audio_url": { "url": "data:audio/wav;base64,BBBB" }
            },
            {
                "type": "video_url",
                "video_url": {
                    "url": "https://example.test/video.mp4",
                    "detail": "low"
                }
            }
        ])))
        .expect("media should parse");

        assert_eq!(parts[0], InputPart::Text("describe".to_string()));
        assert_eq!(
            parts[1],
            InputPart::Image(MediaPart {
                source: MediaSource::DataUrl {
                    mime_type: "image/png".to_string(),
                    data: "AAAA".to_string(),
                },
                detail: Some("high".to_string()),
            })
        );
        assert_eq!(
            parts[2],
            InputPart::Audio(MediaPart {
                source: MediaSource::DataUrl {
                    mime_type: "audio/wav".to_string(),
                    data: "BBBB".to_string(),
                },
                detail: None,
            })
        );
        assert_eq!(
            parts[3],
            InputPart::Video(MediaPart {
                source: MediaSource::Url("https://example.test/video.mp4".to_string()),
                detail: Some("low".to_string()),
            })
        );
        assert!(parts.iter().any(is_media_part));
        assert!(parts.iter().any(is_non_text_part));
        assert_eq!(to_lossy_text(&parts), "describe");
    }

    #[test]
    fn parses_native_input_image_file_id() {
        let parts = parse_openai_chat_content(Some(&json!([{
            "type": "input_image",
            "file_id": "file_123",
            "detail": "low"
        }])))
        .expect("input_image should parse");

        assert_eq!(
            parts,
            vec![InputPart::Image(MediaPart {
                source: MediaSource::FileId("file_123".to_string()),
                detail: Some("low".to_string()),
            })]
        );
    }

    #[test]
    fn rejects_media_part_missing_url() {
        let error = parse_openai_chat_content(Some(&json!([{
            "type": "image_url",
            "image_url": { "detail": "high" }
        }])))
        .expect_err("missing url should fail");

        assert!(error.to_string().contains("missing url"));
    }

    #[test]
    fn rejects_invalid_data_url() {
        let error = parse_openai_chat_content(Some(&json!([{
            "type": "audio_url",
            "audio_url": { "url": "data:audio/wav;base64," }
        }])))
        .expect_err("empty data URL should fail");

        assert!(error.to_string().contains("invalid data URL"));
    }

    #[test]
    fn preserves_unknown_parts() {
        let value = json!({
            "type": "provider_magic",
            "payload": { "x": true }
        });
        let parts =
            parse_openai_chat_content(Some(&json!([value.clone()]))).expect("unknown should parse");

        assert_eq!(
            parts,
            vec![InputPart::Unknown {
                ty: Some("provider_magic".to_string()),
                value,
            }]
        );
        assert!(!parts.iter().any(is_media_part));
        assert!(parts.iter().any(is_non_text_part));
    }

    #[test]
    fn preserves_provider_native_parts_before_text_fallback() {
        for value in [
            json!({ "inlineData": "bad", "text": "fallback" }),
            json!({ "file_data": "bad", "content": "fallback" }),
            json!({ "function_call": "bad", "text": "fallback" }),
            json!({ "functionResponse": "bad", "content": "fallback" }),
        ] {
            let parts = parse_openai_chat_content(Some(&json!([value.clone()])))
                .expect("native marker should parse");

            assert_eq!(parts, vec![InputPart::Unknown { ty: None, value }]);
            assert_eq!(to_lossy_text(&parts), "fallback");
        }
    }

    #[test]
    fn text_only_provider_rejects_media_parts() {
        let messages = json!([
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "describe" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                ]
            }
        ]);

        let error = reject_media_for_text_only_provider(
            "OpenAI-compatible text completions",
            Some(&messages),
        )
        .expect_err("text-only provider should reject media");

        assert!(error.to_string().contains("cannot preserve image input"));
    }

    #[test]
    fn text_only_provider_rejects_single_object_media_content() {
        let messages = json!([{
            "role": "user",
            "content": {
                "type": "video_url",
                "video_url": { "url": "data:video/mp4;base64,AAAA" }
            }
        }]);

        let error = reject_media_for_text_only_provider("text-only provider", Some(&messages))
            .expect_err("single object media should fail fast");

        assert!(error.to_string().contains("cannot preserve video input"));
    }

    #[test]
    fn text_only_provider_rejects_native_media_part_types() {
        for (part, kind) in [
            (json!({ "type": "image", "source": {} }), "image"),
            (json!({ "type": "audio", "source": {} }), "audio"),
            (json!({ "type": "video", "source": {} }), "video"),
            (json!({ "type": "input_audio", "input_audio": {} }), "audio"),
            (json!({ "type": "input_video", "input_video": {} }), "video"),
            (
                json!({ "type": "input_file", "file_id": "file_123" }),
                "file",
            ),
            (
                json!({ "inlineData": { "mimeType": "image/png", "data": "AAAA" } }),
                "image",
            ),
            (
                json!({ "inline_data": { "mime_type": "audio/wav", "data": "AAAA" } }),
                "audio",
            ),
            (
                json!({ "inlineData": { "mimeType": "Video/MP4", "data": "AAAA" } }),
                "video",
            ),
            (
                json!({ "fileData": { "mimeType": "image/png", "fileUri": "gs://bucket/cat.png" } }),
                "file",
            ),
        ] {
            let messages = json!([{ "role": "user", "content": [part] }]);
            let error = reject_media_for_text_only_provider("text-only provider", Some(&messages))
                .expect_err("native media should fail fast");

            assert!(
                error
                    .to_string()
                    .contains(&format!("cannot preserve {kind} input"))
            );
        }
    }

    #[test]
    fn lossy_text_keeps_unknown_text_fields() {
        let parts = parse_openai_chat_content(Some(&json!([
            "a",
            { "type": "unknown_text", "text": "b" },
            { "content": "c" },
            { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
        ])))
        .expect("parts should parse");

        assert_eq!(to_lossy_text(&parts), "abc");
    }

    fn is_media_part(part: &InputPart) -> bool {
        matches!(
            part,
            InputPart::Image(_) | InputPart::Audio(_) | InputPart::Video(_)
        )
    }

    fn is_non_text_part(part: &InputPart) -> bool {
        !matches!(part, InputPart::Text(_))
    }
}
