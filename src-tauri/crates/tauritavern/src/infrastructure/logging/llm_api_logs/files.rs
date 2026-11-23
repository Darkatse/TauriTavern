use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};

use tt_contracts::observability::LlmApiLogIndexEntryDto;

use super::types::LlmApiLogMeta;

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

pub(super) async fn create_sse_file(log_root: &Path, id: u64) -> std::io::Result<BufWriter<File>> {
    tokio::fs::create_dir_all(log_root).await?;
    let file = File::create(response_raw_sse_path(log_root, id)).await?;
    Ok(BufWriter::new(file))
}

/// One event per line. A line break inside an event can only be whitespace between JSON
/// tokens, so a space takes its place.
pub(super) async fn append_sse_event(
    file: &mut BufWriter<File>,
    event: &str,
) -> std::io::Result<()> {
    for (index, piece) in event.split('\n').enumerate() {
        if index > 0 {
            file.write_all(b" ").await?;
        }
        file.write_all(piece.as_bytes()).await?;
    }
    file.write_all(b"\n").await
}

/// Lines that are not JSON objects, `[DONE]` among them, carry nothing to merge.
pub(super) fn sse_events(raw: &str) -> impl Iterator<Item = Map<String, Value>> + '_ {
    raw.lines()
        .filter_map(|line| serde_json::from_str::<Map<String, Value>>(line).ok())
}

pub(super) async fn persist_raw_files(
    log_root: &Path,
    id: u64,
    request_raw: &str,
    response_json: Option<&str>,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;
    tokio::fs::write(request_raw_path(log_root, id), request_raw).await?;
    if let Some(content) = response_json {
        tokio::fs::write(response_raw_json_path(log_root, id), content).await?;
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
    index_snapshot: &[LlmApiLogIndexEntryDto],
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;
    let content = serde_json::to_string_pretty(index_snapshot).map_err(|error| {
        std::io::Error::other(format!("Failed to serialize LLM API index: {error}"))
    })?;
    tokio::fs::write(index_path(log_root), content).await?;
    Ok(())
}
