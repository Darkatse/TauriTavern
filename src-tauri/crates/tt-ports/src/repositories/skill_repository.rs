use async_trait::async_trait;

use tt_domain::errors::DomainError;
use tt_domain::models::skill::{
    SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
    SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest, SkillReadResult,
    SkillScope, SkillScopeFilter, SkillScopeRetargetRequest, SkillScopeRetargetResult,
    SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
};

#[async_trait]
pub trait SkillRepository: Send + Sync {
    async fn list_skills(
        &self,
        scope_filter: SkillScopeFilter,
    ) -> Result<Vec<SkillIndexEntry>, DomainError>;

    async fn list_skill_files(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<Vec<SkillFileRef>, DomainError>;

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

    async fn search_skill_files(
        &self,
        request: SkillSearchRequest,
    ) -> Result<SkillSearchResult, DomainError>;

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
