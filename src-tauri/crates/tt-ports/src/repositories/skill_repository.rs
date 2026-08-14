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

    /// 解析已安装 skill 包内文件的**规范化绝对物理路径**。
    /// 实现必须校验：skill 已安装、相对路径规范化后未逃逸 skill 目录、
    /// 目标存在且为普通文件（非符号链接）。
    async fn skill_file_path(
        &self,
        scope: SkillScope,
        name: &str,
        relative_path: &str,
    ) -> Result<std::path::PathBuf, DomainError>;

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
