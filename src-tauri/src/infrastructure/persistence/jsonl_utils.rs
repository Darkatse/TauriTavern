use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;
use serde_json::Value;
use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

/// Read a JSONL file and parse it into a vector of JSON values
///
/// # Arguments
///
/// * `path` - The path to the JSONL file
///
/// # Returns
///
/// * `Ok(Vec<Value>)` - The parsed JSON values
/// * `Err(DomainError)` - If the file cannot be read or parsed
pub async fn read_jsonl_file(path: &Path) -> Result<Vec<Value>, DomainError> {
    logger::debug(&format!("Reading JSONL file: {:?}", path));

    // Open the file
    let file = File::open(path).await.map_err(|e| {
        logger::error(&format!("Failed to open JSONL file: {}", e));
        DomainError::InternalError(format!("Failed to open JSONL file: {}", e))
    })?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    parse_jsonl_lines(&mut lines).await
}

/// Parse JSONL payload bytes into JSON values.
pub fn parse_jsonl_bytes(bytes: &[u8]) -> Result<Vec<Value>, DomainError> {
    let text = std::str::from_utf8(bytes).map_err(|e| {
        DomainError::InvalidData(format!("JSONL payload is not valid UTF-8: {}", e))
    })?;
    let mut objects = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(line) {
            Ok(obj) => objects.push(obj),
            Err(e) => logger::warn(&format!("Failed to parse JSON line: {}", e)),
        }
    }
    Ok(objects)
}

/// Read the first non-empty line from a JSONL file.
pub async fn read_first_non_empty_jsonl_line(path: &Path) -> Result<Option<String>, DomainError> {
    let file = File::open(path).await.map_err(|e| {
        logger::error(&format!("Failed to open JSONL file: {}", e));
        DomainError::InternalError(format!("Failed to open JSONL file: {}", e))
    })?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await.map_err(|e| {
        logger::error(&format!("Failed to read line from JSONL file: {}", e));
        DomainError::InternalError(format!("Failed to read line from JSONL file: {}", e))
    })? {
        if !line.trim().is_empty() {
            return Ok(Some(line));
        }
    }

    Ok(None)
}

/// Write a vector of JSON values to a JSONL file
///
/// # Arguments
///
/// * `path` - The path to the JSONL file
/// * `objects` - The JSON values to write
///
/// # Returns
///
/// * `Ok(())` - If the file was written successfully
/// * `Err(DomainError)` - If the file cannot be written
pub async fn write_jsonl_file(path: &Path, objects: &[Value]) -> Result<(), DomainError> {
    logger::debug(&format!("Writing JSONL file: {:?}", path));

    let mut serialized = Vec::new();

    for obj in objects {
        let line = serde_json::to_string(obj).map_err(|e| {
            logger::error(&format!("Failed to serialize JSON: {}", e));
            DomainError::InternalError(format!("Failed to serialize JSON: {}", e))
        })?;
        serialized.extend_from_slice(line.as_bytes());
        serialized.push(b'\n');
    }

    write_jsonl_bytes_file(path, &serialized).await
}

/// Atomically write raw JSONL bytes to a file.
pub async fn write_jsonl_bytes_file(path: &Path, bytes: &[u8]) -> Result<(), DomainError> {
    let temp_path = path.with_extension("jsonl.tmp");

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await.map_err(|e| {
                logger::error(&format!("Failed to create directory: {}", e));
                DomainError::InternalError(format!("Failed to create directory: {}", e))
            })?;
        }
    }

    let file = File::create(&temp_path).await.map_err(|e| {
        logger::error(&format!("Failed to create temporary file: {}", e));
        DomainError::InternalError(format!("Failed to create temporary file: {}", e))
    })?;

    let mut writer = BufWriter::new(file);
    writer.write_all(bytes).await.map_err(|e| {
        logger::error(&format!("Failed to write to temporary file: {}", e));
        DomainError::InternalError(format!("Failed to write to temporary file: {}", e))
    })?;

    writer.flush().await.map_err(|e| {
        logger::error(&format!("Failed to flush temporary file: {}", e));
        DomainError::InternalError(format!("Failed to flush temporary file: {}", e))
    })?;

    fs::rename(&temp_path, path).await.map_err(|e| {
        logger::error(&format!("Failed to rename temporary file: {}", e));
        DomainError::InternalError(format!("Failed to rename temporary file: {}", e))
    })?;

    Ok(())
}

async fn parse_jsonl_lines<R>(lines: &mut tokio::io::Lines<R>) -> Result<Vec<Value>, DomainError>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut objects = Vec::new();

    while let Some(line) = lines.next_line().await.map_err(|e| {
        logger::error(&format!("Failed to read line from JSONL file: {}", e));
        DomainError::InternalError(format!("Failed to read line from JSONL file: {}", e))
    })? {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(&line) {
            Ok(obj) => objects.push(obj),
            Err(e) => logger::warn(&format!("Failed to parse JSON line: {}", e)),
        }
    }

    Ok(objects)
}
