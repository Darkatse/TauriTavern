use std::path::{Path, PathBuf};

use super::types::{LlmApiLogIndexEntry, LlmApiLogMeta, LlmApiRawKind};

pub(super) fn index_path(log_root: &Path) -> PathBuf {
    log_root.join("llm-api-index.json")
}

pub(super) fn meta_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.meta.json"))
}

pub(super) fn request_raw_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.request.json"))
}

pub(super) fn response_raw_json_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.response.json"))
}

pub(super) fn response_raw_sse_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.response.sse"))
}

pub(super) async fn load_meta(path: PathBuf) -> Result<LlmApiLogMeta, std::io::Error> {
    let content = tokio::fs::read_to_string(path).await?;
    serde_json::from_str::<LlmApiLogMeta>(&content)
        .map_err(|error| std::io::Error::other(format!("Failed to parse meta JSON: {error}")))
}

pub(super) async fn persist_meta_file(
    log_root: &Path,
    meta: &LlmApiLogMeta,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;
    let meta_json = serde_json::to_string_pretty(meta).map_err(|error| {
        std::io::Error::other(format!("Failed to serialize LLM API meta: {error}"))
    })?;
    tokio::fs::write(meta_path(log_root, meta.id), meta_json).await?;
    Ok(())
}

pub(super) async fn persist_raw_files(
    log_root: &Path,
    id: u64,
    request_raw_inline: Option<&str>,
    response_raw_kind: Option<LlmApiRawKind>,
    response_raw_inline: Option<&str>,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;

    if let Some(content) = request_raw_inline {
        tokio::fs::write(request_raw_path(log_root, id), content).await?;
    }

    if let Some(kind) = response_raw_kind {
        if let Some(content) = response_raw_inline {
            match kind {
                LlmApiRawKind::Json => {
                    tokio::fs::write(response_raw_json_path(log_root, id), content).await?;
                }
                LlmApiRawKind::Sse => {
                    tokio::fs::write(response_raw_sse_path(log_root, id), content).await?;
                }
            }
        }
    }

    Ok(())
}

pub(super) async fn delete_entry_files(log_root: &Path, id: u64) -> Result<(), std::io::Error> {
    for path in [
        meta_path(log_root, id),
        request_raw_path(log_root, id),
        response_raw_json_path(log_root, id),
        response_raw_sse_path(log_root, id),
    ] {
        if tokio::fs::remove_file(&path).await.is_err() {
            // Missing files are already absent from the retention set.
        }
    }
    Ok(())
}

pub(super) async fn persist_index_file(
    log_root: &Path,
    index_snapshot: &[LlmApiLogIndexEntry],
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;
    let content = serde_json::to_string_pretty(index_snapshot).map_err(|error| {
        std::io::Error::other(format!("Failed to serialize LLM API index: {error}"))
    })?;
    tokio::fs::write(index_path(log_root), content).await?;
    Ok(())
}
