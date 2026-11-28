use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use url::Url;
use zip::write::SimpleFileOptions as FileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::infrastructure::paths::RuntimePaths;
use crate::infrastructure::zipkit::export_file_options;
use tt_contracts::observability::REDACTED;
use tt_domain::errors::DomainError;

const BUNDLE_ROOT_DIR: &str = "tauritavern-dev-bundle";

const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const FILE_IO_BUFFER_BYTES: usize = 1024 * 1024;

/// Compared against key names lowercased with separators removed, so `proxy_password`,
/// `apiKey` and `custom_include_headers` all match.
const CREDENTIAL_KEY_SUFFIXES: [&str; 13] = [
    "password",
    "passwd",
    "secret",
    "apikey",
    "accesskey",
    "secretkey",
    "privatekey",
    "token",
    "auth",
    "credential",
    "credentials",
    "header",
    "headers",
];

pub struct DevLogBundleInput {
    pub meta_json: String,
    pub readme_text: String,
    pub frontend_logs_jsonl: String,
    pub backend_logs_tail_text: String,
}

pub fn export_dev_log_bundle(
    output_path: PathBuf,
    runtime_paths: &RuntimePaths,
    input: DevLogBundleInput,
) -> Result<PathBuf, DomainError> {
    let output_file = File::create(&output_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create export bundle file {}: {}",
            output_path.display(),
            error
        ))
    })?;
    let buffered_output = BufWriter::with_capacity(FILE_IO_BUFFER_BYTES, output_file);
    let mut writer = ZipWriter::new(buffered_output);

    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);

    writer
        .add_directory(format!("{}/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add bundle root: {error}"))
        })?;

    add_text_file(
        &mut writer,
        &format!("{}/meta.json", BUNDLE_ROOT_DIR),
        &input.meta_json,
    )?;
    add_text_file(
        &mut writer,
        &format!("{}/README.txt", BUNDLE_ROOT_DIR),
        &input.readme_text,
    )?;

    writer
        .add_directory(format!("{}/frontend/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add frontend directory: {error}"))
        })?;
    add_text_file(
        &mut writer,
        &format!("{}/frontend/logs.jsonl", BUNDLE_ROOT_DIR),
        &input.frontend_logs_jsonl,
    )?;

    writer
        .add_directory(format!("{}/backend/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add backend directory: {error}"))
        })?;
    add_text_file(
        &mut writer,
        &format!("{}/backend/tail.txt", BUNDLE_ROOT_DIR),
        &input.backend_logs_tail_text,
    )?;

    writer
        .add_directory(format!("{}/settings/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add settings directory: {error}"))
        })?;

    let settings_dir = runtime_paths.data_root.join("default-user");
    for file_name in [
        "tauritavern-settings.json",
        "settings.json",
        "settings/appearance.json",
        "settings/dynamic-theme.json",
        "settings/presets.json",
        "settings/layout.json",
    ] {
        let source_path = settings_dir.join(file_name);
        if !source_path.is_file() {
            continue;
        }

        let zip_path = format!("{}/settings/{}", BUNDLE_ROOT_DIR, file_name);
        add_text_file(&mut writer, &zip_path, &read_redacted_json(&source_path)?)?;
    }

    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];

    for source_path in list_log_root_files(&runtime_paths.log_root, |name| {
        name.starts_with("tauritavern.log")
    })? {
        let file_name = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Invalid backend log file name: {}",
                    source_path.display()
                ))
            })?;

        let zip_path = format!("{}/backend/{}", BUNDLE_ROOT_DIR, file_name);
        add_file_from_disk(&mut writer, &source_path, &zip_path, &mut copy_buffer)?;
    }

    writer
        .add_directory(format!("{}/llm-api/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add llm-api directory: {error}"))
        })?;

    for source_path in
        list_log_root_files(&runtime_paths.log_root, |name| name.starts_with("llm-api-"))?
    {
        let file_name = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Invalid LLM API log file name: {}",
                    source_path.display()
                ))
            })?;

        let zip_path = format!("{}/llm-api/{}", BUNDLE_ROOT_DIR, file_name);
        // A request's headers are recorded in its meta; payloads are copied as they are.
        if file_name.ends_with(".meta.json") {
            add_text_file(&mut writer, &zip_path, &read_redacted_json(&source_path)?)?;
        } else {
            add_file_from_disk(&mut writer, &source_path, &zip_path, &mut copy_buffer)?;
        }
    }

    let mut buffered_output = writer.finish().map_err(|error| {
        DomainError::InternalError(format!("Failed to finalize bundle: {error}"))
    })?;
    buffered_output.flush().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to flush export bundle {}: {}",
            output_path.display(),
            error
        ))
    })?;

    Ok(output_path)
}

/// The copy keeps no value under a credential-like key and no user info in a URL.
fn read_redacted_json(path: &Path) -> Result<String, DomainError> {
    let bytes = fs::read(path).map_err(|error| {
        DomainError::InternalError(format!("Failed to read {}: {}", path.display(), error))
    })?;
    let mut document: Value = serde_json::from_slice(&bytes).map_err(|error| {
        DomainError::InvalidData(format!("{} is not valid JSON: {}", path.display(), error))
    })?;
    redact_credentials(&mut document);
    serde_json::to_string_pretty(&document).map_err(|error| {
        DomainError::InternalError(format!("Failed to serialize {}: {}", path.display(), error))
    })
}

fn redact_credentials(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if is_credential_key(key) && holds_content(field) {
                    *field = Value::String(REDACTED.to_string());
                } else {
                    redact_credentials(field);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_credentials),
        Value::String(text) => strip_url_credentials(text),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn is_credential_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    CREDENTIAL_KEY_SUFFIXES
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
}

/// Flags and numbers under credential-like names, such as `add_bos_token`, stay readable.
fn holds_content(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn strip_url_credentials(text: &mut String) {
    if !text.contains('@') {
        return;
    }
    let Ok(mut url) = Url::parse(text) else {
        return;
    };
    if url.username().is_empty() && url.password().is_none() {
        return;
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    *text = url.into();
}

fn add_text_file(
    writer: &mut ZipWriter<impl Write + Seek>,
    zip_path: &str,
    content: &str,
) -> Result<(), DomainError> {
    writer
        .start_file(zip_path, export_file_options(zip_path))
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add {zip_path}: {error}"))
        })?;

    writer.write_all(content.as_bytes()).map_err(|error| {
        DomainError::InternalError(format!("Failed to write archive entry {zip_path}: {error}"))
    })?;

    Ok(())
}

fn add_file_from_disk(
    writer: &mut ZipWriter<impl Write + Seek>,
    source_path: &Path,
    zip_path: &str,
    copy_buffer: &mut [u8],
) -> Result<(), DomainError> {
    let file_options = export_file_options(source_path);

    writer.start_file(zip_path, file_options).map_err(|error| {
        DomainError::InternalError(format!("Failed to add {zip_path} to archive: {error}"))
    })?;

    let mut source_file = File::open(source_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to open source file {}: {}",
            source_path.display(),
            error
        ))
    })?;

    loop {
        let bytes_read = source_file.read(copy_buffer).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read source file {}: {}",
                source_path.display(),
                error
            ))
        })?;
        if bytes_read == 0 {
            break;
        }

        writer
            .write_all(&copy_buffer[..bytes_read])
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write archive entry {zip_path}: {error}"
                ))
            })?;
    }

    Ok(())
}

fn list_log_root_files(
    log_root: &Path,
    predicate: impl Fn(&str) -> bool,
) -> Result<Vec<PathBuf>, DomainError> {
    let mut files = Vec::new();

    let entries = fs::read_dir(log_root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read log directory {}: {}",
            log_root.display(),
            error
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read log directory entry {}: {}",
                log_root.display(),
                error
            ))
        })?;

        let file_type = entry.file_type().map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read log directory entry type {}: {}",
                entry.path().display(),
                error
            ))
        })?;

        if !file_type.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !predicate(&file_name) {
            continue;
        }

        files.push(entry.path());
    }

    files.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .map(|value| value.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(""));
        let right_name = right
            .file_name()
            .map(|value| value.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(""));
        left_name.cmp(&right_name)
    });
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use serde_json::{Value, json};

    use super::{DevLogBundleInput, export_dev_log_bundle, redact_credentials};
    use crate::infrastructure::paths::{RuntimeMode, RuntimePaths};

    #[test]
    fn exported_llm_api_meta_keeps_no_request_header_values() {
        let root = tempfile::tempdir().unwrap();
        let runtime_paths = RuntimePaths::new(RuntimeMode::Standard, root.path().to_path_buf());
        std::fs::create_dir_all(&runtime_paths.log_root).unwrap();
        let meta = json!({
            "id": 7,
            "endpoint": "https://api.example.com/v1/chat/completions",
            "http": {
                "transport": "http",
                "method": "POST",
                "url": "https://api.example.com/v1/chat/completions",
                "headers": [{ "name": "x-trace", "value": "user-value" }]
            }
        });
        let request = r#"{"model":"gpt-test","headers":"part of the prompt"}"#;
        std::fs::write(
            runtime_paths.log_root.join("llm-api-7.meta.json"),
            meta.to_string(),
        )
        .unwrap();
        std::fs::write(
            runtime_paths.log_root.join("llm-api-7.request.json"),
            request,
        )
        .unwrap();

        let bundle = export_dev_log_bundle(
            root.path().join("bundle.zip"),
            &runtime_paths,
            DevLogBundleInput {
                meta_json: String::new(),
                readme_text: String::new(),
                frontend_logs_jsonl: String::new(),
                backend_logs_tail_text: String::new(),
            },
        )
        .unwrap();

        let mut archive = zip::ZipArchive::new(std::fs::File::open(bundle).unwrap()).unwrap();
        let mut read = |name: &str| {
            let mut content = String::new();
            archive
                .by_name(&format!("tauritavern-dev-bundle/llm-api/{name}"))
                .unwrap()
                .read_to_string(&mut content)
                .unwrap();
            content
        };
        let exported_meta: Value = serde_json::from_str(&read("llm-api-7.meta.json")).unwrap();
        assert_eq!(exported_meta["http"]["headers"], "[redacted]");
        assert_eq!(exported_meta["http"]["url"], meta["http"]["url"]);
        assert_eq!(read("llm-api-7.request.json"), request);
    }

    #[test]
    fn settings_copy_drops_credentials_and_keeps_other_values() {
        let mut settings = json!({
            "proxies": [{ "name": "Relay", "url": "https://user:pass@relay.example/v1", "password": "p" }],
            "selected_proxy": { "name": "Relay", "password": "p" },
            "oai_settings": {
                "proxy_password": "p",
                "custom_include_headers": "Authorization: Bearer k",
                "vertexai_auth_mode": "express",
                "openai_max_tokens": 300
            },
            "extension_settings": {
                "apiKey": "k",
                "sd": { "auto_auth": "user:pass" },
                "third-party": { "accessToken": "k", "endpoint": { "headers": { "X-Key": "k" } } }
            },
            "textgenerationwebui_settings": { "add_bos_token": true, "banned_tokens": "a" },
            "request_proxy": { "url": "http://user:pass@127.0.0.1:7890" },
            "empty": { "password": "" }
        });

        redact_credentials(&mut settings);

        assert_eq!(
            settings,
            json!({
                "proxies": [{ "name": "Relay", "url": "https://relay.example/v1", "password": "[redacted]" }],
                "selected_proxy": { "name": "Relay", "password": "[redacted]" },
                "oai_settings": {
                    "proxy_password": "[redacted]",
                    "custom_include_headers": "[redacted]",
                    "vertexai_auth_mode": "express",
                    "openai_max_tokens": 300
                },
                "extension_settings": {
                    "apiKey": "[redacted]",
                    "sd": { "auto_auth": "[redacted]" },
                    "third-party": { "accessToken": "[redacted]", "endpoint": { "headers": "[redacted]" } }
                },
                "textgenerationwebui_settings": { "add_bos_token": true, "banned_tokens": "a" },
                "request_proxy": { "url": "http://127.0.0.1:7890/" },
                "empty": { "password": "" }
            })
        );
    }
}
