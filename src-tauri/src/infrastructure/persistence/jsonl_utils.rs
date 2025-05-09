use std::path::Path;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use serde_json::Value;
use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;

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

/// Append a JSON value to a JSONL file
///
/// # Arguments
///
/// * `path` - The path to the JSONL file
/// * `object` - The JSON value to append
///
/// # Returns
///
/// * `Ok(())` - If the value was appended successfully
/// * `Err(DomainError)` - If the file cannot be written
pub async fn append_jsonl_file(path: &Path, object: &Value) -> Result<(), DomainError> {
    logger::debug(&format!("Appending to JSONL file: {:?}", path));
    
    // Create the parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await.map_err(|e| {
                logger::error(&format!("Failed to create directory: {}", e));
                DomainError::InternalError(format!("Failed to create directory: {}", e))
            })?;
        }
    }
    
    // Open the file in append mode
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to open file for appending: {}", e));
            DomainError::InternalError(format!("Failed to open file for appending: {}", e))
        })?;
    
    let mut writer = BufWriter::new(file);
    
    // Serialize the object to JSON
    let line = serde_json::to_string(object).map_err(|e| {
        logger::error(&format!("Failed to serialize JSON: {}", e));
        DomainError::InternalError(format!("Failed to serialize JSON: {}", e))
    })?;
    
    // Write the JSON line
    writer.write_all(line.as_bytes()).await.map_err(|e| {
        logger::error(&format!("Failed to write to file: {}", e));
        DomainError::InternalError(format!("Failed to write to file: {}", e))
    })?;
    
    writer.write_all(b"\n").await.map_err(|e| {
        logger::error(&format!("Failed to write newline to file: {}", e));
        DomainError::InternalError(format!("Failed to write newline to file: {}", e))
    })?;
    
    // Flush the writer to ensure all data is written
    writer.flush().await.map_err(|e| {
        logger::error(&format!("Failed to flush file: {}", e));
        DomainError::InternalError(format!("Failed to flush file: {}", e))
    })?;
    
    Ok(())
}

/// Read the first line of a file
///
/// # Arguments
///
/// * `path` - The path to the file
///
/// # Returns
///
/// * `Ok(String)` - The first line of the file
/// * `Err(DomainError)` - If the file cannot be read
pub async fn read_first_line(path: &Path) -> Result<String, DomainError> {
    logger::debug(&format!("Reading first line from file: {:?}", path));
    
    // Open the file
    let file = File::open(path).await.map_err(|e| {
        logger::error(&format!("Failed to open file: {}", e));
        DomainError::InternalError(format!("Failed to open file: {}", e))
    })?;
    
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    
    // Read the first line
    let bytes_read = reader.read_line(&mut line).await.map_err(|e| {
        logger::error(&format!("Failed to read line from file: {}", e));
        DomainError::InternalError(format!("Failed to read line from file: {}", e))
    })?;
    
    if bytes_read == 0 {
        return Err(DomainError::InvalidData("File is empty".to_string()));
    }
    
    Ok(line.trim().to_string())
}

/// Read the last line of a file
///
/// # Arguments
///
/// * `path` - The path to the file
///
/// # Returns
///
/// * `Ok(String)` - The last line of the file
/// * `Err(DomainError)` - If the file cannot be read
pub async fn read_last_line(path: &Path) -> Result<String, DomainError> {
    logger::debug(&format!("Reading last line from file: {:?}", path));
    
    // Read the entire file
    let content = fs::read_to_string(path).await.map_err(|e| {
        logger::error(&format!("Failed to read file: {}", e));
        DomainError::InternalError(format!("Failed to read file: {}", e))
    })?;
    
    // Split the content into lines and get the last non-empty line
    let last_line = content.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| DomainError::InvalidData("File has no non-empty lines".to_string()))?;
    
    Ok(last_line.to_string())
}

/// Check if a JSONL file has the expected chat ID hash
///
/// # Arguments
///
/// * `path` - The path to the JSONL file
/// * `expected_hash` - The expected chat ID hash
///
/// # Returns
///
/// * `Ok(bool)` - Whether the file has the expected chat ID hash
/// * `Err(DomainError)` - If the file cannot be read or parsed
pub async fn check_chat_integrity(path: &Path, expected_hash: u64) -> Result<bool, DomainError> {
    logger::debug(&format!("Checking chat integrity: {:?}", path));
    
    // Read the first line
    let first_line = read_first_line(path).await?;
    
    // Parse the first line as JSON
    let metadata = serde_json::from_str::<Value>(&first_line).map_err(|e| {
        logger::error(&format!("Failed to parse JSON metadata: {}", e));
        DomainError::InvalidData(format!("Failed to parse JSON metadata: {}", e))
    })?;
    
    // Extract the chat ID hash
    let chat_id_hash = metadata["chat_metadata"]["chat_id_hash"].as_u64().unwrap_or(0);
    
    Ok(chat_id_hash == expected_hash)
}
