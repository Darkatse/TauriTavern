mod archive;
mod delete;
mod fs_ops;
mod index;
mod install;
mod layout;
mod manifest;
mod materialize;
mod package;
mod paths;
mod read;
mod retarget;
mod source_refs;
mod write;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::PathBuf;

use async_trait::async_trait;
use tokio::sync::Mutex;

use tt_domain::errors::DomainError;
use tt_domain::models::skill::{
    SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
    SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest, SkillReadResult,
    SkillScope, SkillScopeFilter, SkillScopeRetargetRequest, SkillScopeRetargetResult,
    SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
};
use tt_ports::repositories::skill_repository::SkillRepository;

const INDEX_VERSION: u32 = 2;
const SIDECAR_VERSION: u32 = 1;
const MAX_FILES: usize = 1000;
const MAX_SINGLE_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SKILL_MD_BYTES: u64 = 1024 * 1024;
const MAX_ZIP_COMPRESSION_RATIO: u64 = 100;

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

    pub(super) async fn installed_skill_root(
        &self,
        scope: &SkillScope,
        name: &str,
    ) -> Result<PathBuf, DomainError> {
        let name = paths::validate_skill_name(name)?;
        paths::validate_skill_scope(scope)?;
        let index = self.load_index().await?;
        if !index
            .skills
            .iter()
            .any(|skill| skill.scope == *scope && skill.name == name)
        {
            return Err(DomainError::NotFound(format!(
                "Skill not found: {}/{}",
                scope.label(),
                name
            )));
        }

        let skill_root = self.installed_scope_root(scope)?.join(&name);
        let root_metadata = fs::symlink_metadata(&skill_root).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "Skill directory not found: {}/{}",
                    scope.label(),
                    name
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read Skill directory metadata '{}': {}",
                    skill_root.display(),
                    error
                ))
            }
        })?;
        if root_metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Skill directory cannot be a symlink: {}/{}",
                scope.label(),
                name
            )));
        }
        if !root_metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Skill installed path is not a directory: {}/{}",
                scope.label(),
                name
            )));
        }
        Ok(skill_root)
    }
}

#[async_trait]
impl SkillRepository for FileSkillRepository {
    async fn list_skills(
        &self,
        scope_filter: SkillScopeFilter,
    ) -> Result<Vec<SkillIndexEntry>, DomainError> {
        paths::validate_skill_scope_filter(&scope_filter)?;
        let index = self.load_index_view_filtered_missing_dirs().await?;
        Ok(index
            .skills
            .into_iter()
            .filter(|skill| scope_filter.matches(&skill.scope))
            .collect())
    }

    async fn list_skill_files(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<Vec<SkillFileRef>, DomainError> {
        let skill_root = self.installed_skill_root(&scope, name).await?;
        package::collect_skill_files(&skill_root)
    }

    async fn preview_import(
        &self,
        input: SkillImportInput,
        target_scope: SkillScope,
    ) -> Result<SkillImportPreview, DomainError> {
        let prepared = self.materialize_input(&input).await?;
        let result = self
            .preview_prepared(&prepared, target_scope)
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
        let validated = match self
            .preview_prepared(&prepared, request.target_scope.clone())
            .await
        {
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
        request: SkillReadRequest,
    ) -> Result<SkillReadResult, DomainError> {
        read::read_skill_file(self, request).await
    }

    async fn write_skill_file(
        &self,
        request: SkillWriteRequest,
    ) -> Result<SkillReadResult, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        write::write_skill_file(self, request).await
    }

    async fn search_skill_files(
        &self,
        request: SkillSearchRequest,
    ) -> Result<SkillSearchResult, DomainError> {
        read::search_skill_files(self, request).await
    }

    async fn export_skill(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<SkillExportResult, DomainError> {
        let name = paths::validate_skill_name(name)?;
        paths::validate_skill_scope(&scope)?;
        let index = self.load_index().await?;
        if !index
            .skills
            .iter()
            .any(|skill| skill.scope == scope && skill.name == name)
        {
            return Err(DomainError::NotFound(format!(
                "Skill not found: {}/{}",
                scope.label(),
                name
            )));
        }

        let root = self.installed_scope_root(&scope)?.join(&name);
        let bytes = archive::export_skill_dir(&root)?;
        let sha256 = package::sha256_hex(&bytes);
        Ok(SkillExportResult {
            file_name: format!("{name}.zip"),
            bytes,
            sha256,
        })
    }

    async fn delete_skill(&self, scope: SkillScope, name: &str) -> Result<(), DomainError> {
        let _guard = self.mutation_lock.lock().await;
        delete::delete_skill(self, scope, name).await
    }

    async fn move_skill(
        &self,
        request: SkillMoveRequest,
    ) -> Result<SkillInstallResult, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        delete::move_skill(self, request).await
    }

    async fn retarget_scope(
        &self,
        request: SkillScopeRetargetRequest,
    ) -> Result<SkillScopeRetargetResult, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        retarget::retarget_scope(self, request).await
    }

    async fn delete_skills_for_source(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<Vec<String>, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        delete::delete_skills_for_source(self, source_kind, source_id).await
    }
}
