use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::path::Path;

/// Represents a UI theme in the application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    /// The name of the theme
    pub name: String,
    
    /// The raw theme data as JSON
    pub data: Value,
}

impl Theme {
    /// Create a new theme
    pub fn new(name: String, data: Value) -> Self {
        Self { name, data }
    }
    
    /// Get the filename for this theme
    pub fn get_filename(&self) -> String {
        format!("{}.json", sanitize_filename(&self.name))
    }
}

/// Sanitize a filename to ensure it's safe to use in a file system
/// 
/// This function removes or replaces characters that are not safe for filenames
/// across different operating systems.
pub fn sanitize_filename(filename: &str) -> String {
    // List of characters that are not allowed in filenames across most file systems
    let invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    
    // Replace invalid characters with underscores
    let mut sanitized = filename.to_string();
    for c in invalid_chars {
        sanitized = sanitized.replace(c, "_");
    }
    
    // Trim leading and trailing whitespace
    sanitized = sanitized.trim().to_string();
    
    // If the filename is empty after sanitization, use a default name
    if sanitized.is_empty() {
        sanitized = "unnamed_theme".to_string();
    }
    
    sanitized
}
