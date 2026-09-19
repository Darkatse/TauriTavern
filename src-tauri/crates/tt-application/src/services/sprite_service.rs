use std::path::{Path, PathBuf};
use std::sync::Arc;

use tt_domain::errors::DomainError;
use tt_ports::repositories::sprite_repository::{
    SpriteName, SpriteRepository, SpriteSet, StoredSprite,
};
use url::Url;

use crate::dto::sprite_dto::{
    DeleteSpriteDto, ListSpritesDto, SpriteDto, UploadSpriteDto, UploadSpritePackDto,
};

pub struct SpriteService {
    repository: Arc<dyn SpriteRepository>,
}

impl SpriteService {
    pub fn new(repository: Arc<dyn SpriteRepository>) -> Self {
        Self { repository }
    }

    pub async fn list(&self, dto: ListSpritesDto) -> Result<Vec<SpriteDto>, DomainError> {
        let set = SpriteSet::parse(&dto.name)?;
        let sprites = self.repository.list(&set).await?;
        Ok(sprites
            .into_iter()
            .map(|sprite| sprite_dto(&set, sprite))
            .collect())
    }

    pub async fn upload(&self, dto: UploadSpriteDto) -> Result<(), DomainError> {
        let set = SpriteSet::parse(&dto.name)?;
        let sprite_name = SpriteName::parse(&dto.sprite_name)?;
        let source_path = required_path(&dto.file_path, "Sprite upload path")?;
        self.repository
            .upload(&set, &sprite_name, &dto.original_filename, &source_path)
            .await
    }

    pub async fn upload_pack(&self, dto: UploadSpritePackDto) -> Result<usize, DomainError> {
        let set = SpriteSet::parse(&dto.name)?;
        let archive_path = required_path(&dto.file_path, "Sprite pack path")?;
        self.repository.upload_pack(&set, &archive_path).await
    }

    pub async fn delete(&self, dto: DeleteSpriteDto) -> Result<(), DomainError> {
        let set = SpriteSet::parse(&dto.name)?;
        let sprite_name = SpriteName::parse(&dto.sprite_name)?;
        self.repository.delete(&set, &sprite_name).await
    }
}

fn required_path(value: &str, label: &str) -> Result<PathBuf, DomainError> {
    let path = Path::new(value);
    if path.as_os_str().is_empty() {
        return Err(DomainError::InvalidData(format!("{label} cannot be empty")));
    }
    Ok(path.to_path_buf())
}

fn sprite_dto(set: &SpriteSet, sprite: StoredSprite) -> SpriteDto {
    let label = sprite_label(&sprite.file_name);
    let mut url = Url::parse("http://localhost").expect("constant base URL must parse");
    {
        let mut segments = url
            .path_segments_mut()
            .expect("HTTP base URL supports path segments");
        segments.push("characters");
        for segment in set.segments() {
            segments.push(segment);
        }
        segments.push(&sprite.file_name);
    }

    let mut path = url.path().to_string();
    if let Some(modified_at) = sprite.modified_at {
        path.push_str("?t=");
        path.push_str(&modified_at.format("%Y%m%d%H%M%S").to_string());
    }

    SpriteDto { label, path }
}

fn sprite_label(file_name: &str) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name)
        .to_lowercase();
    let suffix = stem
        .char_indices()
        .skip(1)
        .find_map(|(index, character)| matches!(character, '-' | '.').then_some(index));
    suffix.map_or(stem.clone(), |index| stem[..index].to_string())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn sprite_dto_preserves_upstream_labels_and_encodes_client_paths() {
        let set = SpriteSet::parse("Alice/formal wear").expect("sprite set");
        let sprite = sprite_dto(
            &set,
            StoredSprite {
                file_name: "Joy.Expressive #1.png".to_string(),
                modified_at: Some(Utc.with_ymd_and_hms(2026, 8, 12, 13, 14, 15).unwrap()),
            },
        );
        assert_eq!(
            sprite,
            SpriteDto {
                label: "joy".to_string(),
                path: "/characters/Alice/formal%20wear/Joy.Expressive%20%231.png?t=20260812131415"
                    .to_string(),
            }
        );
    }

    #[test]
    fn label_separator_must_follow_at_least_one_character() {
        assert_eq!(sprite_label("joy.png"), "joy");
        assert_eq!(sprite_label("joy-1.webp"), "joy");
        assert_eq!(sprite_label("joy.expressive.gif"), "joy");
        assert_eq!(sprite_label(".joy.png"), ".joy");
    }
}
