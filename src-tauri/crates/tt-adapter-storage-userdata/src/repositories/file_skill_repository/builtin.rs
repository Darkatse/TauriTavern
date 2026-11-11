use std::fs;

use serde_json::Value;

use super::{FileSkillRepository, package};
use tt_domain::errors::DomainError;
use tt_domain::models::skill::{SkillIndexEntry, SkillScope};

impl FileSkillRepository {
    pub(super) async fn builtin_entries(&self) -> Result<&[SkillIndexEntry], DomainError> {
        if let Some(entries) = self.builtin_entries.get() {
            return Ok(entries);
        }
        let root = self.builtin_root.clone();
        let entries = tokio::task::spawn_blocking(move || {
            let directories = fs::read_dir(&root)
                .map_err(|error| DomainError::file_io("read", root.display().to_string(), error))?;
            let mut entries = Vec::new();
            for directory in directories {
                let directory = directory.map_err(|error| {
                    DomainError::file_io("read", root.display().to_string(), error)
                })?;
                let path = directory.path();
                let metadata = directory.file_type().map_err(|error| {
                    DomainError::file_io("stat", path.display().to_string(), error)
                })?;
                if !metadata.is_dir() {
                    continue;
                }
                let mut entry = package::validate_skill_root(&path, Value::Null)?.entry;
                if directory.file_name() != entry.name.as_str() {
                    return Err(DomainError::InvalidData(format!(
                        "Built-in Skill directory '{}' does not match SKILL.md name '{}'",
                        path.display(),
                        entry.name
                    )));
                }
                entry.scope = SkillScope::Builtin;
                entries.push(entry);
            }
            entries.sort_by(|left, right| left.name.cmp(&right.name));
            Ok::<_, DomainError>(entries)
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Built-in Skill scan task failed: {error}"))
        })??;
        Ok(self.builtin_entries.get_or_init(|| entries))
    }
}
