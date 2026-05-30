use std::collections::BTreeMap;
use std::sync::Arc;

use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::AgentSkillPolicy;
use crate::domain::models::skill::{
    SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
    SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest, SkillReadResult,
    SkillScope, SkillScopeFilter, SkillScopeRetargetRequest, SkillScopeRetargetResult,
    SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
};
use crate::domain::repositories::skill_repository::SkillRepository;

pub struct SkillService {
    repository: Arc<dyn SkillRepository>,
}

impl SkillService {
    pub fn new(repository: Arc<dyn SkillRepository>) -> Self {
        Self { repository }
    }

    pub async fn list_skills(
        &self,
        scope_filter: SkillScopeFilter,
    ) -> Result<Vec<SkillIndexEntry>, ApplicationError> {
        Ok(self.repository.list_skills(scope_filter).await?)
    }

    pub async fn list_skill_files(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<Vec<SkillFileRef>, ApplicationError> {
        Ok(self.repository.list_skill_files(scope, name).await?)
    }

    pub async fn preview_import(
        &self,
        input: SkillImportInput,
        target_scope: SkillScope,
    ) -> Result<SkillImportPreview, ApplicationError> {
        Ok(self.repository.preview_import(input, target_scope).await?)
    }

    pub async fn install_import(
        &self,
        request: SkillInstallRequest,
    ) -> Result<SkillInstallResult, ApplicationError> {
        Ok(self.repository.install_import(request).await?)
    }

    pub async fn read_skill_file(
        &self,
        request: SkillReadRequest,
    ) -> Result<SkillReadResult, ApplicationError> {
        Ok(self.repository.read_skill_file(request).await?)
    }

    pub async fn write_skill_file(
        &self,
        request: SkillWriteRequest,
    ) -> Result<SkillReadResult, ApplicationError> {
        Ok(self.repository.write_skill_file(request).await?)
    }

    pub async fn search_skill_files(
        &self,
        request: SkillSearchRequest,
    ) -> Result<SkillSearchResult, ApplicationError> {
        Ok(self.repository.search_skill_files(request).await?)
    }

    pub async fn export_skill(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<SkillExportResult, ApplicationError> {
        Ok(self.repository.export_skill(scope, name).await?)
    }

    pub async fn delete_skill(
        &self,
        scope: SkillScope,
        name: &str,
    ) -> Result<(), ApplicationError> {
        Ok(self.repository.delete_skill(scope, name).await?)
    }

    pub async fn move_skill(
        &self,
        request: SkillMoveRequest,
    ) -> Result<SkillInstallResult, ApplicationError> {
        Ok(self.repository.move_skill(request).await?)
    }

    pub async fn retarget_scope(
        &self,
        request: SkillScopeRetargetRequest,
    ) -> Result<SkillScopeRetargetResult, ApplicationError> {
        Ok(self.repository.retarget_scope(request).await?)
    }

    pub async fn resolve_effective_skills(
        &self,
        scope_order: &[SkillScope],
        policy: &AgentSkillPolicy,
    ) -> Result<Vec<SkillIndexEntry>, ApplicationError> {
        let installed = self.repository.list_skills(SkillScopeFilter::All).await?;
        let mut by_scope = BTreeMap::new();
        for skill in installed {
            by_scope.insert((skill.scope.stable_key(), skill.name.clone()), skill);
        }

        let mut effective = BTreeMap::<String, SkillIndexEntry>::new();
        for scope in scope_order {
            for ((scope_key, name), skill) in &by_scope {
                if scope_key == &scope.stable_key() {
                    effective.insert(name.clone(), skill.clone());
                }
            }
        }

        if !policy.visible.iter().any(|name| name == "*") {
            for name in &policy.visible {
                if !effective.contains_key(name) {
                    return Err(ApplicationError::ValidationError(format!(
                        "agent.skill_visible_missing: Skill `{name}` is explicitly visible in the profile but is not installed in the active Skill scopes"
                    )));
                }
            }
        }

        Ok(effective
            .into_values()
            .filter(|skill| skill_is_visible(policy, skill.name.as_str()))
            .collect())
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

fn skill_is_visible(policy: &AgentSkillPolicy, name: &str) -> bool {
    if policy
        .deny
        .iter()
        .any(|denied| denied == "*" || denied == name)
    {
        return false;
    }
    policy
        .visible
        .iter()
        .any(|visible| visible == "*" || visible == name)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use chrono::Utc;

    use super::*;
    use crate::domain::errors::DomainError;
    use crate::domain::models::agent::profile::AgentSkillPolicy;
    use crate::domain::models::skill::{
        SkillExportResult, SkillImportInput, SkillImportPreview, SkillInstallRequest,
        SkillMoveRequest, SkillReadRequest, SkillReadResult, SkillScopeRetargetRequest,
        SkillScopeRetargetResult, SkillSearchRequest, SkillSearchResult, SkillSourceRef,
        SkillWriteRequest,
    };
    use crate::domain::repositories::skill_repository::SkillRepository;

    struct FakeSkillRepository {
        skills: Vec<SkillIndexEntry>,
    }

    #[async_trait]
    impl SkillRepository for FakeSkillRepository {
        async fn list_skills(
            &self,
            scope_filter: SkillScopeFilter,
        ) -> Result<Vec<SkillIndexEntry>, DomainError> {
            Ok(self
                .skills
                .iter()
                .filter(|skill| scope_filter.matches(&skill.scope))
                .cloned()
                .collect())
        }

        async fn list_skill_files(
            &self,
            _scope: SkillScope,
            _name: &str,
        ) -> Result<Vec<crate::domain::models::skill::SkillFileRef>, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn preview_import(
            &self,
            _input: SkillImportInput,
            _target_scope: SkillScope,
        ) -> Result<SkillImportPreview, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn install_import(
            &self,
            _request: SkillInstallRequest,
        ) -> Result<crate::domain::models::skill::SkillInstallResult, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn read_skill_file(
            &self,
            _request: SkillReadRequest,
        ) -> Result<SkillReadResult, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn write_skill_file(
            &self,
            _request: SkillWriteRequest,
        ) -> Result<SkillReadResult, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn search_skill_files(
            &self,
            _request: SkillSearchRequest,
        ) -> Result<SkillSearchResult, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn export_skill(
            &self,
            _scope: SkillScope,
            _name: &str,
        ) -> Result<SkillExportResult, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn delete_skill(&self, _scope: SkillScope, _name: &str) -> Result<(), DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn move_skill(
            &self,
            _request: SkillMoveRequest,
        ) -> Result<crate::domain::models::skill::SkillInstallResult, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn retarget_scope(
            &self,
            _request: SkillScopeRetargetRequest,
        ) -> Result<SkillScopeRetargetResult, DomainError> {
            unreachable!("not needed for resolver tests")
        }

        async fn delete_skills_for_source(
            &self,
            _source_kind: &str,
            _source_id: &str,
        ) -> Result<Vec<String>, DomainError> {
            unreachable!("not needed for resolver tests")
        }
    }

    fn policy(visible: Vec<&str>, deny: Vec<&str>) -> AgentSkillPolicy {
        AgentSkillPolicy {
            visible: visible.into_iter().map(str::to_string).collect(),
            deny: deny.into_iter().map(str::to_string).collect(),
            max_read_chars_per_call: 1000,
            max_read_chars_per_run: 1000,
        }
    }

    fn skill(scope: SkillScope, name: &str, hash: &str) -> SkillIndexEntry {
        SkillIndexEntry {
            scope,
            name: name.to_string(),
            description: format!("{name} skill"),
            display_name: None,
            source_kind: None,
            license: None,
            author: None,
            version: None,
            tags: Vec::new(),
            installed_hash: hash.to_string(),
            file_count: 1,
            total_bytes: 1,
            has_scripts: false,
            has_binary: false,
            installed_at: Utc::now(),
            source_refs: Vec::<SkillSourceRef>::new(),
        }
    }

    #[tokio::test]
    async fn resolve_effective_skills_prefers_later_scopes() {
        let service = SkillService::new(Arc::new(FakeSkillRepository {
            skills: vec![
                skill(SkillScope::Global, "writer", "global"),
                skill(
                    SkillScope::Profile {
                        profile_id: "profile-a".to_string(),
                    },
                    "writer",
                    "profile",
                ),
            ],
        }));

        let resolved = service
            .resolve_effective_skills(
                &[
                    SkillScope::Global,
                    SkillScope::Profile {
                        profile_id: "profile-a".to_string(),
                    },
                ],
                &policy(vec!["*"], vec![]),
            )
            .await
            .expect("resolve skills");

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].installed_hash, "profile");
    }

    #[tokio::test]
    async fn resolve_effective_skills_fails_when_explicit_visible_skill_is_missing() {
        let service = SkillService::new(Arc::new(FakeSkillRepository { skills: Vec::new() }));

        let error = service
            .resolve_effective_skills(&[SkillScope::Global], &policy(vec!["writer"], vec![]))
            .await
            .expect_err("missing explicit skill should fail");

        assert!(error.to_string().contains("agent.skill_visible_missing"));
    }
}
