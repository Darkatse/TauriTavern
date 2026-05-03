use std::sync::Arc;

use crate::application::errors::ApplicationError;
use crate::domain::models::skill::{
    SkillExportResult, SkillImportInput, SkillImportPreview, SkillIndexEntry, SkillInstallRequest,
    SkillInstallResult, SkillReadResult,
};
use crate::domain::repositories::skill_repository::SkillRepository;

pub struct SkillService {
    repository: Arc<dyn SkillRepository>,
}

impl SkillService {
    pub fn new(repository: Arc<dyn SkillRepository>) -> Self {
        Self { repository }
    }

    pub async fn list_skills(&self) -> Result<Vec<SkillIndexEntry>, ApplicationError> {
        Ok(self.repository.list_skills().await?)
    }

    pub async fn preview_import(
        &self,
        input: SkillImportInput,
    ) -> Result<SkillImportPreview, ApplicationError> {
        Ok(self.repository.preview_import(input).await?)
    }

    pub async fn install_import(
        &self,
        request: SkillInstallRequest,
    ) -> Result<SkillInstallResult, ApplicationError> {
        Ok(self.repository.install_import(request).await?)
    }

    pub async fn read_skill_file(
        &self,
        name: &str,
        path: &str,
        max_chars: Option<usize>,
    ) -> Result<SkillReadResult, ApplicationError> {
        Ok(self
            .repository
            .read_skill_file(name, path, max_chars)
            .await?)
    }

    pub async fn export_skill(&self, name: &str) -> Result<SkillExportResult, ApplicationError> {
        Ok(self.repository.export_skill(name).await?)
    }

    pub async fn delete_skills_for_source(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<Vec<String>, ApplicationError> {
        Ok(self
            .repository
            .delete_skills_for_source(source_kind, source_id)
            .await?)
    }
}
