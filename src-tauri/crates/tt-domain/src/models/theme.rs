use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}
