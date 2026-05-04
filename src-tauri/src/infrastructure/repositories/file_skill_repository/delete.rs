use super::FileSkillRepository;
use super::fs_ops::delete_installed_skill_dir;
use super::paths::validate_skill_name;
use crate::domain::errors::DomainError;

pub(super) async fn delete_skill(
    repository: &FileSkillRepository,
    name: &str,
) -> Result<(), DomainError> {
    let name = validate_skill_name(name)?;
    let mut index = repository.load_index().await?;
    let Some(position) = index.skills.iter().position(|skill| skill.name == name) else {
        return Err(DomainError::NotFound(format!("Skill not found: {name}")));
    };

    delete_installed_skill_dir(&repository.installed_root().join(&name), &name)?;
    index.skills.remove(position);
    repository.save_index(&index).await
}
