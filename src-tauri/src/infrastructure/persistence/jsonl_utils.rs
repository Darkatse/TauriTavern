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
    let mut objects = Vec::new();

    // Read each line and parse it as JSON
    while let Some(line) = lines.next_line().await.map_err(|e| {
        logger::error(&format!("Failed to read line from JSONL file: {}", e));
        DomainError::InternalError(format!("Failed to read line from JSONL file: {}", e))
    })? {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(&line) {
            Ok(obj) => objects.push(obj),
            Err(e) => {
                logger::warn(&format!("Failed to parse JSON line: {}", e));
                // Continue reading other lines
            }
        }
    }

    Ok(objects)
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

    // Create a temporary file
    let temp_path = path.with_extension("jsonl.tmp");

    // Create the parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await.map_err(|e| {
                logger::error(&format!("Failed to create directory: {}", e));
                DomainError::InternalError(format!("Failed to create directory: {}", e))
            })?;
        }
    }

    // Open the temporary file
    let file = File::create(&temp_path).await.map_err(|e| {
        logger::error(&format!("Failed to create temporary file: {}", e));
        DomainError::InternalError(format!("Failed to create temporary file: {}", e))
    })?;

    let mut writer = BufWriter::new(file);

    // Write each object as a JSON line
    for obj in objects {
        let line = serde_json::to_string(obj).map_err(|e| {
            logger::error(&format!("Failed to serialize JSON: {}", e));
            DomainError::InternalError(format!("Failed to serialize JSON: {}", e))
        })?;

        writer.write_all(line.as_bytes()).await.map_err(|e| {
            logger::error(&format!("Failed to write to temporary file: {}", e));
            DomainError::InternalError(format!("Failed to write to temporary file: {}", e))
        })?;

        writer.write_all(b"\n").await.map_err(|e| {
            logger::error(&format!("Failed to write newline to temporary file: {}", e));
            DomainError::InternalError(format!("Failed to write newline to temporary file: {}", e))
        })?;
    }

    // Flush the writer to ensure all data is written
    writer.flush().await.map_err(|e| {
        logger::error(&format!("Failed to flush temporary file: {}", e));
        DomainError::InternalError(format!("Failed to flush temporary file: {}", e))
    })?;

    // Rename the temporary file to the target file (atomic operation)
    fs::rename(&temp_path, path).await.map_err(|e| {
        logger::error(&format!("Failed to rename temporary file: {}", e));
        DomainError::InternalError(format!("Failed to rename temporary file: {}", e))
    })?;

    Ok(())
}
