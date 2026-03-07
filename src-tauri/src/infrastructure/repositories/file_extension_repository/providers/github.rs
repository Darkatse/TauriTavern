use bytes::Bytes;
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::domain::errors::DomainError;

use super::{ExtensionSourceProvider, parse_bytes_or_error, parse_json_or_error, split_owner_repo};
use crate::infrastructure::repositories::file_extension_repository::repo_url::HOST_GITHUB;

const GITHUB_API_BASE: &str = "https://api.github.com";

#[derive(Debug, Deserialize)]
struct GithubRepositoryInfo {
    default_branch: String,
}

#[derive(Debug, Deserialize)]
struct GithubCommit {
    sha: String,
}

pub(super) struct GithubProvider {
    http_client: Client,
}

impl GithubProvider {
    pub(super) fn new(http_client: Client) -> Self {
        Self { http_client }
    }

    fn build_api_url(&self, segments: &[&str]) -> Result<Url, DomainError> {
        let mut url = Url::parse(GITHUB_API_BASE).map_err(|error| {
            DomainError::InternalError(format!("Failed to parse GitHub API base URL: {}", error))
        })?;

        {
            let mut path_segments = url.path_segments_mut().map_err(|_| {
                DomainError::InternalError("Failed to mutate GitHub API URL".to_string())
            })?;
            path_segments.clear();
            for segment in segments {
                path_segments.push(segment);
            }
        }

        Ok(url)
    }
}

#[async_trait::async_trait]
impl ExtensionSourceProvider for GithubProvider {
    fn host(&self) -> &'static str {
        HOST_GITHUB
    }

    async fn default_branch(&self, repo_path: &str) -> Result<String, DomainError> {
        let (owner, repo) = split_owner_repo(repo_path, self.host())?;
        let url = self.build_api_url(&["repos", owner, repo])?;

        let response = self
            .http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("GitHub request failed: {}", error))
            })?;

        let info: GithubRepositoryInfo = parse_json_or_error(response, &url, "GitHub").await?;
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
        let url = self.build_api_url(&["repos", owner, repo, "commits", reference])?;

        let response = self
            .http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("GitHub request failed: {}", error))
            })?;

        let commit: GithubCommit = parse_json_or_error(response, &url, "GitHub").await?;
        if commit.sha.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}' returned an empty commit SHA for reference '{}'",
                repo_path, reference
            )));
        }

        Ok(commit.sha)
    }

    async fn download_archive_zip(
        &self,
        repo_path: &str,
        commit: &str,
    ) -> Result<Bytes, DomainError> {
        let (owner, repo) = split_owner_repo(repo_path, self.host())?;
        let url = self.build_api_url(&["repos", owner, repo, "zipball", commit])?;

        let response = self
            .http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to download extension archive: {}",
                    error
                ))
            })?;

        parse_bytes_or_error(response, &url, "GitHub").await
    }
}
