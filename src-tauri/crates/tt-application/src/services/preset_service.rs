use std::sync::Arc;
use tt_domain::errors::DomainError;
use tt_domain::json_document::JsonDocument;
use tt_domain::models::preset::PresetType;
use tt_ports::repositories::preset_repository::PresetRepository;

/// Service for managing presets
pub struct PresetService {
    preset_repository: Arc<dyn PresetRepository>,
}

impl PresetService {
    /// Create a new PresetService
    pub fn new(preset_repository: Arc<dyn PresetRepository>) -> Self {
        Self { preset_repository }
    }

    pub async fn save_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
        preset_json: &str,
    ) -> Result<(), DomainError> {
        tracing::debug!("Saving preset: {} (type: {})", name, preset_type);

        let preset = JsonDocument::parse(preset_json.as_bytes())
            .map_err(|error| DomainError::InvalidData(format!("Invalid preset JSON: {error}")))?;
        self.preset_repository
            .save_preset(name, preset_type, preset)
            .await?;

        tracing::debug!("Preset saved successfully: {}", name);
        Ok(())
    }

    /// Delete a preset
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the preset to delete
    /// * `preset_type` - Type of the preset
    ///
    /// # Returns
    ///
    /// * `Result<(), DomainError>` - Success or error
    pub async fn delete_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<(), DomainError> {
        tracing::debug!("Deleting preset: {} (type: {})", name, preset_type);

        if !self
            .preset_repository
            .preset_exists(name, preset_type)
            .await?
        {
            return Err(DomainError::NotFound(format!("Preset not found: {}", name)));
        }

        // Delete the preset
        self.preset_repository
            .delete_preset(name, preset_type)
            .await?;

        tracing::debug!("Preset deleted successfully: {}", name);
        Ok(())
    }

    /// List all presets of a specific type
    ///
    /// # Arguments
    ///
    /// * `preset_type` - Type of presets to list
    ///
    /// # Returns
    ///
    /// * `Result<Vec<String>, DomainError>` - List of preset names
    pub async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        tracing::debug!("Listing presets of type: {}", preset_type);

        let presets = self.preset_repository.list_presets(preset_type).await?;

        tracing::debug!("Found {} presets of type {}", presets.len(), preset_type);

        Ok(presets)
    }

    /// Restore a default preset
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the preset to restore
    /// * `preset_type` - Type of the preset
    ///
    /// # Returns
    ///
    /// * `Result<Option<JsonDocument>, DomainError>` - The bundled document if found, None otherwise
    pub async fn restore_default_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<JsonDocument>, DomainError> {
        tracing::debug!("Restoring default preset: {} (type: {})", name, preset_type);

        let default_preset = self
            .preset_repository
            .get_default_preset(name, preset_type)
            .await?;

        if default_preset.is_some() {
            tracing::debug!("Default preset found for restoration: {}", name);
        } else {
            tracing::debug!("Default preset not found: {}", name);
        }

        Ok(default_preset)
    }

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
    pub async fn preset_exists(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<bool, DomainError> {
        tracing::debug!(
            "Checking if preset exists: {} (type: {})",
            name,
            preset_type
        );

        let exists = self
            .preset_repository
            .preset_exists(name, preset_type)
            .await?;

        tracing::debug!("Preset {} exists: {}", name, exists);

        Ok(exists)
    }
}
