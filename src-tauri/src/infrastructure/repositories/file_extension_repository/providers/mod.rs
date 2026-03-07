use async_trait::async_trait;
use bytes::Bytes;
use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use url::Url;

use crate::domain::errors::DomainError;

use super::repo_url::{HOST_GITEE, HOST_GITHUB, HOST_GITLAB};

pub(super) mod gitee;
pub(super) mod github;
pub(super) mod gitlab;

#[async_trait]
pub(super) trait ExtensionSourceProvider: Send + Sync {
    fn host(&self) -> &'static str;

    async fn default_branch(&self, repo_path: &str) -> Result<String, DomainError>;

    async fn latest_commit(&self, repo_path: &str, reference: &str) -> Result<String, DomainError>;

    async fn download_archive_zip(
        &self,
        repo_path: &str,
        commit: &str,
    ) -> Result<Bytes, DomainError>;
}

pub(super) struct ExtensionSourceProviders {
    github: github::GithubProvider,
    gitlab: gitlab::GitLabProvider,
    gitee: gitee::GiteeProvider,
}

impl ExtensionSourceProviders {
    pub(super) fn new(http_client: Client) -> Self {
        Self {
            github: github::GithubProvider::new(http_client.clone()),
            gitlab: gitlab::GitLabProvider::new(http_client.clone()),
            gitee: gitee::GiteeProvider::new(http_client),
        }
    }

    pub(super) fn for_host(&self, host: &str) -> Result<&dyn ExtensionSourceProvider, DomainError> {
        match host {
            HOST_GITHUB => Ok(&self.github),
            HOST_GITLAB => Ok(&self.gitlab),
            HOST_GITEE => Ok(&self.gitee),
            _ => Err(DomainError::InvalidData(format!(
                "Unsupported extension repository host: {}",
                host
            ))),
        }
    }
}

pub(super) async fn parse_json_or_error<T>(
    response: Response,
    url: &Url,
    provider: &str,
) -> Result<T, DomainError>
where
    T: DeserializeOwned,
{
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let snippet = body.trim();
        let suffix = if snippet.is_empty() {
            String::new()
        } else {
            format!(" ({})", snippet)
        };
        return Err(DomainError::InternalError(format!(
            "{} request failed for '{}': HTTP {}{}",
            provider, url, status, suffix
        )));
    }

    response.json::<T>().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to parse {} response for '{}': {}",
            provider, url, error
        ))
    })
}

pub(super) async fn parse_bytes_or_error(
    response: Response,
    url: &Url,
    provider: &str,
) -> Result<Bytes, DomainError> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let snippet = body.trim();
        let suffix = if snippet.is_empty() {
            String::new()
        } else {
            format!(" ({})", snippet)
        };
        return Err(DomainError::InternalError(format!(
            "{} request failed for '{}': HTTP {}{}",
            provider, url, status, suffix
        )));
    }

    response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read {} response for '{}': {}",
            provider, url, error
        ))
    })
}

pub(super) fn split_owner_repo<'a>(
    repo_path: &'a str,
    host: &str,
) -> Result<(&'a str, &'a str), DomainError> {
    let mut parts = repo_path.split('/');
    let owner = parts.next().unwrap_or_default().trim();
    let repo = parts.next().unwrap_or_default().trim();

    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err(DomainError::InvalidData(format!(
            "Invalid repository path for {}: {}",
            host, repo_path
        )));
    }

    Ok((owner, repo))
}
