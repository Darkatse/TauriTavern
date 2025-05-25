use serde::{Serialize, Deserialize};
use serde_json::Value;
use crate::domain::models::theme::Theme;

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

/// DTO for theme response
#[derive(Debug, Serialize, Deserialize)]
pub struct ThemeDto {
    /// The name of the theme
    pub name: String,
    
    /// The theme data
    pub data: Value,
}

impl From<Theme> for ThemeDto {
    fn from(theme: Theme) -> Self {
        Self {
            name: theme.name,
            data: theme.data,
        }
    }
}
