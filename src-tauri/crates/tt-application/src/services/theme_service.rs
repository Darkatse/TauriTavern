use std::sync::Arc;

use crate::errors::ApplicationError;
use tt_domain::errors::DomainError;
use tt_domain::models::theme::Theme;
use tt_ports::repositories::theme_repository::ThemeRepository;

/// Service for managing themes
pub struct ThemeService {
    theme_repository: Arc<dyn ThemeRepository>,
}

impl ThemeService {
    /// Create a new ThemeService
    pub fn new(theme_repository: Arc<dyn ThemeRepository>) -> Self {
        Self { theme_repository }
    }

    /// Save a theme
    pub async fn save_theme(&self, theme_json: &str) -> Result<(), ApplicationError> {
        let theme = Theme::parse(theme_json)?;
        tracing::info!("Saving theme: {}", theme.name);

        self.theme_repository.save_theme(&theme).await.map_err(|e| {
            tracing::error!("Failed to save theme {}: {}", theme.name, e);
            e.into()
        })
    }

    /// Delete a theme
    pub async fn delete_theme(&self, name: &str) -> Result<(), ApplicationError> {
        tracing::info!("Deleting theme: {}", name);

        if name.is_empty() {
            return Err(ApplicationError::ValidationError(
                "Theme name is required".to_string(),
            ));
        }

        self.theme_repository.delete_theme(name).await.map_err(|e| {
            tracing::error!("Failed to delete theme {}: {}", name, e);
            // Convert NotFound to a more specific error
            match e {
                DomainError::NotFound(_) => {
                    ApplicationError::NotFound(format!("Theme not found: {}", name))
                }
                _ => e.into(),
            }
        })
    }
}
