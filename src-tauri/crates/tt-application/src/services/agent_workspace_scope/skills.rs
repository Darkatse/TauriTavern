use tt_domain::frozen_macros::MAX_EXPANDED_TEXT_BYTES;

use super::*;

impl ScopedWorkspaceFs {
    fn skill_entry(
        &self,
        path: &WorkspacePath,
    ) -> Result<
        (
            &dyn SkillRepository,
            &SkillIndexEntry,
            Option<WorkspacePath>,
        ),
        DomainError,
    > {
        let suffix = path
            .as_str()
            .strip_prefix("skills/")
            .expect("package paths are below the skills root");
        let (name, relative) = match suffix.split_once('/') {
            Some((name, relative)) => (name, Some(WorkspacePath::parse(relative)?)),
            None => (suffix, None),
        };
        let binding = self
            .skill_bindings
            .iter()
            .find(|binding| binding.name == name)
            .ok_or_else(|| {
                DomainError::NotFound(format!("Skill is not available: skills/{name}"))
            })?;
        let repository = self
            .skill_repository
            .as_deref()
            .expect("skill bindings have a repository");
        Ok((repository, binding, relative))
    }

    pub(super) async fn read_skill_file(
        &self,
        path: &WorkspacePath,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError> {
        if path.as_str() == SKILLS_ROOT {
            return Err(DomainError::workspace_path_is_directory(path.as_str()));
        }
        let (repository, binding, relative) = self.skill_entry(path)?;
        let Some(relative) = relative else {
            repository
                .skill_metadata(&binding.scope, &binding.name, None)
                .await?;
            return Err(DomainError::workspace_path_is_directory(path.as_str()));
        };
        let bytes = repository
            .read_skill_bytes(&binding.scope, &binding.name, &relative, usize::MAX)
            .await?;
        let bytes = self.project_skill_bytes(&relative, bytes)?;
        if bytes.len() > maximum_bytes {
            return Err(DomainError::InvalidData(format!(
                "Workspace read exceeds {maximum_bytes} bytes: {}",
                path.as_str()
            )));
        }
        Ok(bytes)
    }

    fn project_skill_bytes(
        &self,
        path: &WorkspacePath,
        bytes: Vec<u8>,
    ) -> Result<Vec<u8>, DomainError> {
        if !path.as_str().starts_with("scripts/")
            && let Ok(text) = std::str::from_utf8(&bytes)
            && let std::borrow::Cow::Owned(text) =
                self.frozen_macros.render(text, MAX_EXPANDED_TEXT_BYTES)?
        {
            return Ok(text.into_bytes());
        }
        Ok(bytes)
    }

    pub(super) async fn skill_metadata(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, DomainError> {
        if path.as_str() == SKILLS_ROOT {
            return Ok(binding_directory_metadata());
        }
        let (repository, binding, relative) = self.skill_entry(path)?;
        let mut metadata = repository
            .skill_metadata(&binding.scope, &binding.name, relative.as_ref())
            .await?;
        if metadata.kind == WorkspaceEntryKind::File
            && relative
                .as_ref()
                .is_some_and(|relative| !relative.as_str().starts_with("scripts/"))
        {
            metadata.bytes = self.read_skill_file(path, usize::MAX).await?.len() as u64;
        }
        Ok(metadata)
    }

    pub(super) async fn read_skill_dir(
        &self,
        path: &WorkspacePath,
        maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        if path.as_str() == SKILLS_ROOT {
            if self.skill_bindings.len() > maximum_entries {
                return Err(DomainError::InvalidData(format!(
                    "Workspace directory exceeds {maximum_entries} entries: skills"
                )));
            }
            let mut entries = self
                .skill_bindings
                .iter()
                .map(|binding| {
                    Ok(WorkspaceDirectoryEntry {
                        path: WorkspacePath::parse(format!("skills/{}", binding.name))?,
                        metadata: binding_directory_metadata(),
                    })
                })
                .collect::<Result<Vec<_>, DomainError>>()?;
            entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
            return Ok(entries);
        }
        let (repository, binding, relative) = self.skill_entry(path)?;
        let mut entries = repository
            .read_skill_dir(
                &binding.scope,
                &binding.name,
                relative.as_ref(),
                maximum_entries,
            )
            .await?;
        for entry in &mut entries {
            let project_size = entry.metadata.kind == WorkspaceEntryKind::File
                && !entry.path.as_str().starts_with("scripts/");
            entry.path =
                WorkspacePath::parse(format!("skills/{}/{}", binding.name, entry.path.as_str()))?;
            if project_size {
                entry.metadata.bytes =
                    self.read_skill_file(&entry.path, usize::MAX).await?.len() as u64;
            }
        }
        Ok(entries)
    }
}

fn binding_directory_metadata() -> WorkspaceMetadata {
    WorkspaceMetadata {
        kind: WorkspaceEntryKind::Directory,
        bytes: 0,
        modified: None,
        created: None,
    }
}
