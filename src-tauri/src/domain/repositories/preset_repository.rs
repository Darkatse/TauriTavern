use crate::domain::errors::DomainError;
use crate::domain::models::preset::{DefaultPreset, Preset, PresetType};
use async_trait::async_trait;

/// Repository interface for preset management
#[async_trait]
pub trait PresetRepository: Send + Sync {
    /// Save a preset to the appropriate directory based on its type
    ///
    /// # Arguments
    ///
    /// * `preset` - The preset to save
    ///
    /// # Returns
    ///
    /// * `Result<(), DomainError>` - Success or error
    async fn save_preset(&self, preset: &Preset) -> Result<(), DomainError>;

    /// Delete a preset by name and type
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the preset to delete
    /// * `preset_type` - Type of the preset
    ///
    /// # Returns
    ///
    /// * `Result<(), DomainError>` - Success or error
    async fn delete_preset(&self, name: &str, preset_type: &PresetType) -> Result<(), DomainError>;

    /// Check if a preset exists
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the preset
    /// * `preset_type` - Type of the preset
    ///
    /// # Returns
    ///
    /// * `Result<bool, DomainError>` - True if preset exists, false otherwise
    async fn preset_exists(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<bool, DomainError>;

    /// Get a preset by name and type
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the preset
    /// * `preset_type` - Type of the preset
    ///
    /// # Returns
    ///
    /// * `Result<Option<Preset>, DomainError>` - The preset if found, None otherwise
    async fn get_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<Preset>, DomainError>;

    /// List all presets of a specific type
    ///
    /// # Arguments
    ///
    /// * `preset_type` - Type of presets to list
    ///
    /// # Returns
    ///
    /// * `Result<Vec<String>, DomainError>` - List of preset names
    async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError>;

    /// Get a default preset by name and type from the content system
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the default preset
    /// * `preset_type` - Type of the preset
    ///
    /// # Returns
    ///
    /// * `Result<Option<DefaultPreset>, DomainError>` - The default preset if found, None otherwise
    async fn get_default_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<DefaultPreset>, DomainError>;
}
