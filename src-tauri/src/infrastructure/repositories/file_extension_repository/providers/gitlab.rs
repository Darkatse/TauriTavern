use bytes::Bytes;
use serde::Deserialize;
use std::sync::Arc;
use url::Url;

use crate::domain::errors::DomainError;
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};

use super::{ExtensionSourceProvider, parse_bytes_or_error, parse_json_or_error};
use crate::infrastructure::repositories::file_extension_repository::repo_url::HOST_GITLAB;

const GITLAB_API_BASE: &str = "https://gitlab.com/api/v4";

#[derive(Debug, Deserialize)]
struct GitLabProjectInfo {
    default_branch: String,
}

#[derive(Debug, Deserialize)]
struct GitLabCommit {
    id: String,
}

pub(super) struct GitLabProvider {
    http_clients: Arc<HttpClientPool>,
}

impl GitLabProvider {
    pub(super) fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    fn encode_project_id(repo_path: &str) -> String {
        let mut encoded = String::with_capacity(repo_path.len() + 8);
        for byte in repo_path.as_bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(*byte as char)
                }
                b'/' => encoded.push_str("%2F"),
                _ => encoded.push_str(&format!("%{:02X}", byte)),
            }
        }
        encoded
    }

    fn project_base_url(&self, repo_path: &str) -> Result<Url, DomainError> {
        let project_id = Self::encode_project_id(repo_path);
        Url::parse(&format!("{}/projects/{}", GITLAB_API_BASE, project_id)).map_err(|error| {
            DomainError::InternalError(format!("Failed to build GitLab API URL: {}", error))
        })
    }
}

#[async_trait::async_trait]
impl ExtensionSourceProvider for GitLabProvider {
    fn host(&self) -> &'static str {
        HOST_GITLAB
    }

    async fn default_branch(&self, repo_path: &str) -> Result<String, DomainError> {
        let url = self.project_base_url(repo_path)?;

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client.get(url.clone()).send().await.map_err(|error| {
            DomainError::InternalError(format!("GitLab request failed: {}", error))
        })?;

        let info: GitLabProjectInfo = parse_json_or_error(response, &url, "GitLab").await?;
        if info.default_branch.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}' has no default branch",
                repo_path
            )));
        }

        Ok(info.default_branch)
    }

    async fn latest_commit(&self, repo_path: &str, reference: &str) -> Result<String, DomainError> {
        let mut url = self.project_base_url(repo_path)?;
        url.set_path(&format!("{}/repository/commits", url.path()));
        url.query_pairs_mut()
            .append_pair("ref_name", reference)
            .append_pair("per_page", "1");

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client.get(url.clone()).send().await.map_err(|error| {
            DomainError::InternalError(format!("GitLab request failed: {}", error))
        })?;

        let commits: Vec<GitLabCommit> = parse_json_or_error(response, &url, "GitLab").await?;
        let commit = commits.first().ok_or_else(|| {
            DomainError::InternalError(format!(
                "Repository '{}' returned no commits for reference '{}'",
                repo_path, reference
            ))
        })?;

        if commit.id.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}' returned an empty commit id for reference '{}'",
                repo_path, reference
            )));
        }

        Ok(commit.id.clone())
    }

    async fn download_archive_zip(
        &self,
        repo_path: &str,
        commit: &str,
    ) -> Result<Bytes, DomainError> {
        let mut url = self.project_base_url(repo_path)?;
        url.set_path(&format!("{}/repository/archive.zip", url.path()));
        url.query_pairs_mut().append_pair("sha", commit);

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client
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

        parse_bytes_or_error(response, &url, "GitLab").await
    }
}
