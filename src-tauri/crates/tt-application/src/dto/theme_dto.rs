use serde::{Deserialize, Serialize};

/// DTO for deleting a theme
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteThemeDto {
    /// The name of the theme to delete
    pub name: String,
}
