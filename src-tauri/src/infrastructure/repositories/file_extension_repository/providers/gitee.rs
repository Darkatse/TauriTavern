use bytes::Bytes;
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::domain::errors::DomainError;

use super::{ExtensionSourceProvider, parse_bytes_or_error, parse_json_or_error, split_owner_repo};
use crate::infrastructure::repositories::file_extension_repository::repo_url::HOST_GITEE;

#[derive(Debug, Deserialize)]
struct GiteeRepoInfo {
    #[serde(rename = "default_branch", alias = "defaultBranch")]
    default_branch: String,
}

#[derive(Debug, Deserialize)]
struct GiteeCommit {
    sha: String,
}

pub(super) struct GiteeProvider {
    http_client: Client,
}

impl GiteeProvider {
    pub(super) fn new(http_client: Client) -> Self {
        Self { http_client }
    }

    fn build_api_url(&self, segments: &[&str]) -> Result<Url, DomainError> {
        let mut url = Url::parse("https://gitee.com").map_err(|error| {
            DomainError::InternalError(format!("Failed to parse Gitee API base URL: {}", error))
        })?;

        {
            let mut path_segments = url.path_segments_mut().map_err(|_| {
                DomainError::InternalError("Failed to mutate Gitee API URL".to_string())
            })?;
            path_segments.clear();
            path_segments.push("api");
            path_segments.push("v5");
            for segment in segments {
                path_segments.push(segment);
            }
        }

        Ok(url)
    }
}

#[async_trait::async_trait]
impl ExtensionSourceProvider for GiteeProvider {
    fn host(&self) -> &'static str {
        HOST_GITEE
    }

    async fn default_branch(&self, repo_path: &str) -> Result<String, DomainError> {
        let (owner, repo) = split_owner_repo(repo_path, self.host())?;
        let url = self.build_api_url(&["repos", owner, repo])?;

        let response = self
            .http_client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| DomainError::InternalError(format!("Gitee request failed: {}", error)))?;

        let info: GiteeRepoInfo = parse_json_or_error(response, &url, "Gitee").await?;
        if info.default_branch.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}' has no default branch",
                repo_path
            )));
        }

        Ok(info.default_branch)
    }

    async fn latest_commit(&self, repo_path: &str, reference: &str) -> Result<String, DomainError> {
        let (owner, repo) = split_owner_repo(repo_path, self.host())?;
        let mut url = self.build_api_url(&["repos", owner, repo, "commits"])?;
        url.query_pairs_mut()
            .append_pair("sha", reference)
            .append_pair("page", "1")
            .append_pair("per_page", "1");

        let response = self
            .http_client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| DomainError::InternalError(format!("Gitee request failed: {}", error)))?;

        let commits: Vec<GiteeCommit> = parse_json_or_error(response, &url, "Gitee").await?;
        let commit = commits.first().ok_or_else(|| {
            DomainError::InternalError(format!(
                "Repository '{}' returned no commits for reference '{}'",
                repo_path, reference
            ))
        })?;

        if commit.sha.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}' returned an empty commit SHA for reference '{}'",
                repo_path, reference
            )));
        }

        Ok(commit.sha.clone())
    }

    async fn download_archive_zip(
        &self,
        repo_path: &str,
        commit: &str,
    ) -> Result<Bytes, DomainError> {
        let (owner, repo) = split_owner_repo(repo_path, self.host())?;
        let mut url = self.build_api_url(&["repos", owner, repo, "zipball"])?;
        url.query_pairs_mut().append_pair("ref", commit);

        let response = self
            .http_client
            .get(url.clone())
            .header("Accept", "application/zip")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to download extension archive: {}",
                    error
                ))
            })?;

        parse_bytes_or_error(response, &url, "Gitee").await
    }
}
