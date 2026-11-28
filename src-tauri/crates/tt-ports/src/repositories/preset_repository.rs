use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_domain::json_document::JsonDocument;
use tt_domain::models::preset::PresetType;

/// Repository interface for preset management
#[async_trait]
pub trait PresetRepository: Send + Sync {
    /// The stored document carries `name`, in place when it already has one.
    async fn save_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
        preset: JsonDocument,
    ) -> Result<(), DomainError>;

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
    /// * `Result<Option<JsonDocument>, DomainError>` - The stored document if found, None otherwise
    async fn get_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<JsonDocument>, DomainError>;

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
    /// * `Result<Option<JsonDocument>, DomainError>` - The bundled document if found, None otherwise
    async fn get_default_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<JsonDocument>, DomainError>;
}
