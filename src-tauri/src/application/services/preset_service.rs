use crate::domain::errors::DomainError;
use crate::domain::models::preset::{DefaultPreset, Preset, PresetType};
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::infrastructure::logging::logger;
use std::sync::Arc;

/// Service for managing presets
pub struct PresetService {
    preset_repository: Arc<dyn PresetRepository>,
}

impl PresetService {
    /// Create a new PresetService
    pub fn new(preset_repository: Arc<dyn PresetRepository>) -> Self {
        Self { preset_repository }
    }

    /// Save a preset
    ///
    /// # Arguments
    ///
    /// * `preset` - The preset to save
    ///
    /// # Returns
    ///
    /// * `Result<(), DomainError>` - Success or error
    pub async fn save_preset(&self, preset: &Preset) -> Result<(), DomainError> {
        logger::debug(&format!(
            "Saving preset: {} (type: {})",
            preset.name, preset.preset_type
        ));

        // Validate the preset
        preset.validate().map_err(|e| {
            logger::error(&format!("Preset validation failed: {}", e));
            DomainError::InvalidData(e)
        })?;

        // Save the preset
        self.preset_repository.save_preset(preset).await?;

        logger::info(&format!("Preset saved successfully: {}", preset.name));
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
        logger::debug(&format!(
            "Deleting preset: {} (type: {})",
            name, preset_type
        ));

        // Check if preset exists
        if !self
            .preset_repository
            .preset_exists(name, preset_type)
            .await?
        {
            logger::warn(&format!(
                "Preset not found for deletion: {} (type: {})",
                name, preset_type
            ));
            return Err(DomainError::NotFound(format!("Preset not found: {}", name)));
        }

        // Delete the preset
        self.preset_repository
            .delete_preset(name, preset_type)
            .await?;

        logger::info(&format!("Preset deleted successfully: {}", name));
        Ok(())
    }

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
    pub async fn get_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<Preset>, DomainError> {
        logger::debug(&format!("Getting preset: {} (type: {})", name, preset_type));

        let preset = self.preset_repository.get_preset(name, preset_type).await?;

        if preset.is_some() {
            logger::debug(&format!("Preset found: {}", name));
        } else {
            logger::debug(&format!("Preset not found: {}", name));
        }

        Ok(preset)
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
        logger::debug(&format!("Listing presets of type: {}", preset_type));

        let presets = self.preset_repository.list_presets(preset_type).await?;

        logger::debug(&format!(
            "Found {} presets of type {}",
            presets.len(),
            preset_type
        ));

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
    /// * `Result<Option<DefaultPreset>, DomainError>` - The default preset if found, None otherwise
    pub async fn restore_default_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<DefaultPreset>, DomainError> {
        logger::debug(&format!(
            "Restoring default preset: {} (type: {})",
            name, preset_type
        ));

        let default_preset = self
            .preset_repository
            .get_default_preset(name, preset_type)
            .await?;

        if default_preset.is_some() {
            logger::info(&format!("Default preset found for restoration: {}", name));
        } else {
            logger::debug(&format!("Default preset not found: {}", name));
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
        logger::debug(&format!(
            "Checking if preset exists: {} (type: {})",
            name, preset_type
        ));

        let exists = self
            .preset_repository
            .preset_exists(name, preset_type)
            .await?;

        logger::debug(&format!("Preset {} exists: {}", name, exists));

        Ok(exists)
    }

    /// Create a preset from raw data
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the preset
    /// * `api_id` - API ID string
    /// * `data` - Preset data as JSON value
    ///
    /// # Returns
    ///
    /// * `Result<Preset, DomainError>` - The created preset
    pub fn create_preset(
        &self,
        name: String,
        api_id: &str,
        data: serde_json::Value,
    ) -> Result<Preset, DomainError> {
        logger::debug(&format!("Creating preset: {} (api_id: {})", name, api_id));

        let preset_type = PresetType::from_api_id(api_id).ok_or_else(|| {
            logger::error(&format!("Unknown API ID: {}", api_id));
            DomainError::InvalidData(format!("Unknown API ID: {}", api_id))
        })?;

        let preset = Preset::new(name, preset_type, data);

        // Validate the preset
        preset.validate().map_err(|e| {
            logger::error(&format!("Preset validation failed: {}", e));
            DomainError::InvalidData(e)
        })?;

        logger::debug(&format!("Preset created successfully: {}", preset.name));

        Ok(preset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::preset::PresetType;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;

    // Mock repository for testing
    struct MockPresetRepository {
        presets: std::sync::Mutex<HashMap<(String, PresetType), Preset>>,
    }

    impl MockPresetRepository {
        fn new() -> Self {
            Self {
                presets: std::sync::Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl PresetRepository for MockPresetRepository {
        async fn save_preset(&self, preset: &Preset) -> Result<(), DomainError> {
            let mut presets = self.presets.lock().unwrap();
            presets.insert(
                (preset.name.clone(), preset.preset_type.clone()),
                preset.clone(),
            );
            Ok(())
        }

        async fn delete_preset(
            &self,
            name: &str,
            preset_type: &PresetType,
        ) -> Result<(), DomainError> {
            let mut presets = self.presets.lock().unwrap();
            presets.remove(&(name.to_string(), preset_type.clone()));
            Ok(())
        }

        async fn preset_exists(
            &self,
            name: &str,
            preset_type: &PresetType,
        ) -> Result<bool, DomainError> {
            let presets = self.presets.lock().unwrap();
            Ok(presets.contains_key(&(name.to_string(), preset_type.clone())))
        }

        async fn get_preset(
            &self,
            name: &str,
            preset_type: &PresetType,
        ) -> Result<Option<Preset>, DomainError> {
            let presets = self.presets.lock().unwrap();
            Ok(presets
                .get(&(name.to_string(), preset_type.clone()))
                .cloned())
        }

        async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
            let presets = self.presets.lock().unwrap();
            let names: Vec<String> = presets
                .keys()
                .filter(|(_, t)| t == preset_type)
                .map(|(name, _)| name.clone())
                .collect();
            Ok(names)
        }

        async fn get_default_preset(
            &self,
            _name: &str,
            _preset_type: &PresetType,
        ) -> Result<Option<DefaultPreset>, DomainError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn test_save_and_get_preset() {
        let repository = Arc::new(MockPresetRepository::new());
        let service = PresetService::new(repository);

        let preset = Preset::new(
            "Test Preset".to_string(),
            PresetType::OpenAI,
            json!({"temperature": 0.7}),
        );

        // Save preset
        service.save_preset(&preset).await.unwrap();

        // Get preset
        let retrieved = service
            .get_preset("Test Preset", &PresetType::OpenAI)
            .await
            .unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.name, "Test Preset");
        assert_eq!(retrieved.preset_type, PresetType::OpenAI);
    }

    #[tokio::test]
    async fn test_delete_preset() {
        let repository = Arc::new(MockPresetRepository::new());
        let service = PresetService::new(repository);

        let preset = Preset::new(
            "Test Preset".to_string(),
            PresetType::OpenAI,
            json!({"temperature": 0.7}),
        );

        // Save preset
        service.save_preset(&preset).await.unwrap();

        // Verify it exists
        assert!(service
            .preset_exists("Test Preset", &PresetType::OpenAI)
            .await
            .unwrap());

        // Delete preset
        service
            .delete_preset("Test Preset", &PresetType::OpenAI)
            .await
            .unwrap();

        // Verify it's gone
        assert!(!service
            .preset_exists("Test Preset", &PresetType::OpenAI)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_create_preset() {
        let repository = Arc::new(MockPresetRepository::new());
        let service = PresetService::new(repository);

        let preset = service
            .create_preset(
                "Test Preset".to_string(),
                "openai",
                json!({"temperature": 0.7}),
            )
            .unwrap();

        assert_eq!(preset.name, "Test Preset");
        assert_eq!(preset.preset_type, PresetType::OpenAI);
        assert_eq!(preset.data["temperature"], 0.7);
    }
}
