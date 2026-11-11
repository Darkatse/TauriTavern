use async_trait::async_trait;

use crate::workspace_fs::{WorkspaceDirectoryEntry, WorkspaceMetadata};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;
use tt_domain::models::skill::{
    SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
    SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest, SkillReadResult,
    SkillScope, SkillScopeFilter, SkillScopeRetargetRequest, SkillScopeRetargetResult,
    SkillWriteRequest,
};

#[async_trait]
pub trait SkillRepository: Send + Sync {
    /// Read current installed bytes. Paths are relative to the selected package.
    async fn read_skill_bytes(
        &self,
        scope: &SkillScope,
        name: &str,
        path: &WorkspacePath,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError>;

    /// None denotes the installed package root.
    async fn skill_metadata(
        &self,
        scope: &SkillScope,
        name: &str,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError>;

    /// List one level, sorted by package-relative path; exceeding the limit is an error.
    async fn read_skill_dir(
        &self,
        scope: &SkillScope,
        name: &str,
        path: Option<&WorkspacePath>,
        maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError>;

    async fn list_skills(
        &self,
        scope_filter: SkillScopeFilter,
    ) -> Result<Vec<SkillIndexEntry>, DomainError>;

    async fn list_skill_files(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<Vec<SkillFileRef>, DomainError>;

    async fn discover_imports(
        &self,
        input: SkillImportInput,
    ) -> Result<Vec<SkillImportInput>, DomainError>;

    async fn discard_import_archive(&self, path: &str) -> Result<(), DomainError>;

    async fn preview_import(
        &self,
        input: SkillImportInput,
        target_scope: SkillScope,
    ) -> Result<SkillImportPreview, DomainError>;

    async fn install_import(
        &self,
        request: SkillInstallRequest,
    ) -> Result<SkillInstallResult, DomainError>;

    async fn read_skill_file(
        &self,
        request: SkillReadRequest,
    ) -> Result<SkillReadResult, DomainError>;

    async fn write_skill_file(
        &self,
        request: SkillWriteRequest,
    ) -> Result<SkillReadResult, DomainError>;

    async fn export_skill(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<SkillExportResult, DomainError>;

    async fn delete_skill(&self, scope: SkillScope, name: &str) -> Result<(), DomainError>;

    async fn move_skill(
        &self,
        request: SkillMoveRequest,
    ) -> Result<SkillInstallResult, DomainError>;

    async fn retarget_scope(
        &self,
        request: SkillScopeRetargetRequest,
    ) -> Result<SkillScopeRetargetResult, DomainError>;

    async fn delete_skills_for_source(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<Vec<String>, DomainError>;
}
