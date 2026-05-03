use async_trait::async_trait;

use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillExportResult, SkillImportInput, SkillImportPreview, SkillIndexEntry, SkillInstallRequest,
    SkillInstallResult, SkillReadResult,
};

#[async_trait]
pub trait SkillRepository: Send + Sync {
    async fn list_skills(&self) -> Result<Vec<SkillIndexEntry>, DomainError>;

    async fn preview_import(
        &self,
        input: SkillImportInput,
    ) -> Result<SkillImportPreview, DomainError>;

    async fn install_import(
        &self,
        request: SkillInstallRequest,
    ) -> Result<SkillInstallResult, DomainError>;

    async fn read_skill_file(
        &self,
        name: &str,
        path: &str,
        max_chars: Option<usize>,
    ) -> Result<SkillReadResult, DomainError>;

    async fn export_skill(&self, name: &str) -> Result<SkillExportResult, DomainError>;

    async fn delete_skills_for_source(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<Vec<String>, DomainError>;
}
