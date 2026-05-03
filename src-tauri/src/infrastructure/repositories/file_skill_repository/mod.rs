mod archive;
mod fs_ops;
mod index;
mod install;
mod layout;
mod manifest;
mod materialize;
mod package;
mod paths;
mod read;
mod source_refs;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillExportResult, SkillImportInput, SkillImportPreview, SkillIndexEntry, SkillInstallRequest,
    SkillInstallResult, SkillReadResult,
};
use crate::domain::repositories::skill_repository::SkillRepository;

const INDEX_VERSION: u32 = 1;
const SIDECAR_VERSION: u32 = 1;
const MAX_FILES: usize = 1000;
const MAX_SINGLE_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SKILL_MD_BYTES: u64 = 1024 * 1024;
const MAX_ZIP_COMPRESSION_RATIO: u64 = 100;
const DEFAULT_READ_MAX_CHARS: usize = 20_000;
const MAX_READ_CHARS: usize = 80_000;

pub struct FileSkillRepository {
    pub(super) root: PathBuf,
    mutation_lock: Mutex<()>,
}

impl FileSkillRepository {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            mutation_lock: Mutex::new(()),
        }
    }
}

#[async_trait]
impl SkillRepository for FileSkillRepository {
    async fn list_skills(&self) -> Result<Vec<SkillIndexEntry>, DomainError> {
        Ok(self.load_index().await?.skills)
    }

    async fn preview_import(
        &self,
        input: SkillImportInput,
    ) -> Result<SkillImportPreview, DomainError> {
        let prepared = self.materialize_input(&input).await?;
        let result = self
            .preview_prepared(&prepared)
            .await
            .map(|validated| validated.preview);
        fs_ops::cleanup_dir(&prepared.cleanup_root);
        result
    }

    async fn install_import(
        &self,
        request: SkillInstallRequest,
    ) -> Result<SkillInstallResult, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        let prepared = self.materialize_input(&request.input).await?;
        let validated = match self.preview_prepared(&prepared).await {
            Ok(validated) => validated,
            Err(error) => {
                fs_ops::cleanup_dir(&prepared.cleanup_root);
                return Err(error);
            }
        };
        self.install_validated(prepared, validated, request.conflict_strategy)
            .await
    }

    async fn read_skill_file(
        &self,
        name: &str,
        path: &str,
        max_chars: Option<usize>,
    ) -> Result<SkillReadResult, DomainError> {
        read::read_skill_file(self, name, path, max_chars).await
    }

    async fn export_skill(&self, name: &str) -> Result<SkillExportResult, DomainError> {
        let name = paths::validate_skill_name(name)?;
        let index = self.load_index().await?;
        if !index.skills.iter().any(|skill| skill.name == name) {
            return Err(DomainError::NotFound(format!("Skill not found: {name}")));
        }

        let root = self.installed_root().join(&name);
        let bytes = archive::export_skill_dir(&root)?;
        let sha256 = package::sha256_hex(&bytes);
        Ok(SkillExportResult {
            file_name: format!("{name}.ttskill"),
            bytes,
            sha256,
        })
    }

    async fn delete_skills_for_source(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<Vec<String>, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        source_refs::delete_skills_for_source(self, source_kind, source_id).await
    }
}
