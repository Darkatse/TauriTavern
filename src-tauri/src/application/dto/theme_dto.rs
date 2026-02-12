use serde::{Deserialize, Serialize};
use serde_json::Value;

/// DTO for saving a theme
#[derive(Debug, Serialize, Deserialize)]
pub struct SaveThemeDto {
    /// The name of the theme
    pub name: String,

    /// The theme data
    #[serde(flatten)]
    pub data: Value,
}

/// DTO for deleting a theme
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteThemeDto {
    /// The name of the theme to delete
    pub name: String,
}
