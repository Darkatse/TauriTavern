use serde::{Serialize, Deserialize};

/// Represents a background image in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Background {
    /// The filename of the background image
    pub filename: String,
    
    /// The path to the background image
    pub path: String,
}

impl Background {
    /// Create a new Background instance
    pub fn new(filename: String, path: String) -> Self {
        Self {
            filename,
            path,
        }
    }
}
