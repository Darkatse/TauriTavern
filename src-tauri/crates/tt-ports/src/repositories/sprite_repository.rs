use std::path::Path;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tt_contracts::client_asset_paths::validate_path_segment;
use tt_domain::errors::DomainError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSet {
    segments: Vec<String>,
}

impl SpriteSet {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let segments = value.split('/').map(str::to_string).collect::<Vec<_>>();

        if !(1..=2).contains(&segments.len())
            || segments
                .iter()
                .any(|segment| !validate_path_segment(segment))
        {
            return Err(DomainError::InvalidData(
                "Sprite set must be a name with at most one subfolder".to_string(),
            ));
        }

        Ok(Self { segments })
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteName(String);

impl SpriteName {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        if !validate_path_segment(value) {
            return Err(DomainError::InvalidData("Invalid sprite name".to_string()));
        }

        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSprite {
    pub file_name: String,
    pub modified_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait SpriteRepository: Send + Sync {
    async fn list(&self, set: &SpriteSet) -> Result<Vec<StoredSprite>, DomainError>;

    async fn upload(
        &self,
        set: &SpriteSet,
        sprite_name: &SpriteName,
        original_filename: &str,
        source_path: &Path,
    ) -> Result<(), DomainError>;

    async fn upload_pack(&self, set: &SpriteSet, archive_path: &Path)
    -> Result<usize, DomainError>;

    async fn delete(&self, set: &SpriteSet, sprite_name: &SpriteName) -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::SpriteSet;

    #[test]
    fn sprite_set_accepts_one_optional_subfolder() {
        assert!(SpriteSet::parse("Alice").is_ok());
        assert!(SpriteSet::parse("Alice/formal").is_ok());
        assert!(SpriteSet::parse("Alice/formal/extra").is_err());
        assert!(SpriteSet::parse("../Alice").is_err());
        assert!(SpriteSet::parse("Alice\\formal").is_err());
    }
}
