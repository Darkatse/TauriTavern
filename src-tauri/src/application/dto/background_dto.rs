use crate::domain::models::background::Background;
use serde::{Deserialize, Serialize};

/// DTO for background image response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundDto {
    /// The filename of the background image
    pub filename: String,

    /// The path to the background image
    pub path: String,
}

/// DTO for deleting a background image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteBackgroundDto {
    /// The filename of the background image to delete
    pub bg: String,
}

/// DTO for renaming a background image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameBackgroundDto {
    /// The current filename of the background image
    pub old_bg: String,

    /// The new filename for the background image
    pub new_bg: String,
}

// Implement conversion from domain model to DTO
impl From<Background> for BackgroundDto {
    fn from(background: Background) -> Self {
        Self {
            filename: background.filename,
            path: background.path,
        }
    }
}

// Implement conversion from DTO to domain model
impl From<BackgroundDto> for Background {
    fn from(dto: BackgroundDto) -> Self {
        Self {
            filename: dto.filename,
            path: dto.path,
        }
    }
}
