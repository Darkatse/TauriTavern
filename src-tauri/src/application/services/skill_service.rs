use std::collections::BTreeMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use url::Url;

use crate::application::errors::ApplicationError;
use crate::application::services::external_import_service::{
    DownloadByteLimit, ExternalImportDownloader,
};
#[cfg(test)]
use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::AgentSkillPolicy;
use crate::domain::models::skill::{
    SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
    SkillInlineFile, SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest,
    SkillReadResult, SkillScope, SkillScopeFilter, SkillScopeRetargetRequest,
    SkillScopeRetargetResult, SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
};
use crate::domain::repositories::skill_repository::SkillRepository;

const MAX_REMOTE_SKILL_MD_BYTES: usize = 1024 * 1024;

pub struct SkillService {
    repository: Arc<dyn SkillRepository>,
    external_import_downloader: Arc<dyn ExternalImportDownloader>,
}

impl SkillService {
    #[cfg(test)]
    pub fn new(repository: Arc<dyn SkillRepository>) -> Self {
        Self {
            repository,
            external_import_downloader: Arc::new(UnavailableExternalImportDownloader),
        }
    }

    pub fn with_external_import_downloader(
        repository: Arc<dyn SkillRepository>,
        external_import_downloader: Arc<dyn ExternalImportDownloader>,
    ) -> Self {
        Self {
            repository,
            external_import_downloader,
        }
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

    pub async fn download_import_url(
        &self,
        url: &str,
    ) -> Result<SkillImportInput, ApplicationError> {
        let parsed_url = normalize_skill_import_url(url)?;
        let downloaded = self
            .external_import_downloader
            .fetch_bytes(
                parsed_url.clone(),
                Some(DownloadByteLimit {
                    label: "Remote SKILL.md",
                    max_bytes: MAX_REMOTE_SKILL_MD_BYTES,
                }),
            )
            .await?;
        let bytes = downloaded.bytes;
        let content = String::from_utf8(bytes.clone()).map_err(|_| {
            ApplicationError::ValidationError("Remote SKILL.md must be valid UTF-8".to_string())
        })?;
        let sha256 = sha256_hex(&bytes);
        let source_url = sanitized_source_url(parsed_url);

        Ok(SkillImportInput::InlineFiles {
            files: vec![SkillInlineFile {
                path: "SKILL.md".to_string(),
                encoding: "utf8".to_string(),
                content,
                media_type: Some("text/markdown".to_string()),
                size_bytes: Some(bytes.len() as u64),
                sha256: Some(sha256),
            }],
            source: serde_json::json!({
                "kind": "url",
                "id": source_url,
                "label": source_url,
            }),
        })
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

pub(crate) fn normalize_skill_import_url(raw: &str) -> Result<Url, ApplicationError> {
    let url = Url::parse(raw.trim()).map_err(|_| {
        ApplicationError::ValidationError("Skill import URL must be valid".to_string())
    })?;
    if url.scheme() != "https" {
        return Err(ApplicationError::ValidationError(
            "Skill import URL must use https".to_string(),
        ));
    }
    if url.host_str().is_none() {
        return Err(ApplicationError::ValidationError(
            "Skill import URL host is required".to_string(),
        ));
    }
    let file_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default();
    if file_name != "SKILL.md" {
        return Err(ApplicationError::ValidationError(
            "Skill import URL must point to a raw SKILL.md file".to_string(),
        ));
    }
    Ok(url)
}

pub(crate) fn sanitized_source_url(mut url: Url) -> String {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
struct UnavailableExternalImportDownloader;

#[cfg(test)]
#[async_trait::async_trait]
impl ExternalImportDownloader for UnavailableExternalImportDownloader {
    async fn fetch_bytes(
        &self,
        _url: Url,
        _limit: Option<DownloadByteLimit>,
    ) -> Result<crate::application::services::external_import_service::DownloadedBytes, DomainError>
    {
        Err(DomainError::InternalError(
            "Skill remote import downloader is not configured".to_string(),
        ))
    }

    async fn fetch_to_file(&self, _url: Url, _path: &std::path::Path) -> Result<(), DomainError> {
        Err(DomainError::InternalError(
            "Skill remote import downloader is not configured".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use chrono::Utc;

    use super::*;
    use crate::application::services::external_import_service::DownloadedBytes;
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

    #[tokio::test]
    async fn download_import_url_fetches_skill_md_with_limit_and_sanitized_source() {
        let downloader = Arc::new(FakeExternalImportDownloader {
            bytes: b"# Skill\n\nDo useful work.".to_vec(),
            limit: StdMutex::new(None),
        });
        let service = SkillService::with_external_import_downloader(
            Arc::new(FakeSkillRepository { skills: Vec::new() }),
            downloader.clone(),
        );

        let input = service
            .download_import_url("https://user:secret@example.com/path/SKILL.md?token=1#frag")
            .await
            .expect("download skill");

        let limit = downloader
            .limit
            .lock()
            .unwrap()
            .expect("download byte limit");
        assert_eq!(limit.label, "Remote SKILL.md");
        assert_eq!(limit.max_bytes, 1024 * 1024);
        let SkillImportInput::InlineFiles { files, source } = input else {
            panic!("expected inline skill import");
        };
        assert_eq!(files[0].path, "SKILL.md");
        assert_eq!(files[0].content, "# Skill\n\nDo useful work.");
        assert_eq!(
            files[0].size_bytes,
            Some(b"# Skill\n\nDo useful work.".len() as u64)
        );
        assert_eq!(source["id"], "https://example.com/path/SKILL.md");
    }

    struct FakeExternalImportDownloader {
        bytes: Vec<u8>,
        limit: StdMutex<Option<DownloadByteLimit>>,
    }

    #[async_trait]
    impl ExternalImportDownloader for FakeExternalImportDownloader {
        async fn fetch_bytes(
            &self,
            _url: Url,
            limit: Option<DownloadByteLimit>,
        ) -> Result<DownloadedBytes, DomainError> {
            *self.limit.lock().unwrap() = limit;
            Ok(DownloadedBytes {
                bytes: self.bytes.clone(),
                content_type: Some("text/markdown".to_string()),
                content_disposition: None,
            })
        }

        async fn fetch_to_file(&self, _url: Url, _path: &Path) -> Result<(), DomainError> {
            unimplemented!("not used by these tests")
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
