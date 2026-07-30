use serde::{Deserialize, Serialize};

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
