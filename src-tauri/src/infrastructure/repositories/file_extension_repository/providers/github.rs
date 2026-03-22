use bytes::Bytes;
use reqwest::{Response, StatusCode};
use serde::Deserialize;
use std::sync::Arc;
use url::Url;

use crate::domain::errors::DomainError;
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};

use super::{
    ExtensionSourceProvider, ProviderHttpError, parse_bytes_or_error, parse_json_or_error,
    provider_http_error_to_domain_error, read_provider_http_error, split_owner_repo,
};
use crate::infrastructure::repositories::file_extension_repository::repo_url::HOST_GITHUB;

const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_RATE_LIMIT_TOKENS: [&str; 2] = ["rate limit", "abuse detection"];
const GITHUB_RATE_LIMIT_MESSAGE: &str = "GitHub has rate-limited your requests. Please try again later, or change your network and try again.";

#[derive(Debug, Deserialize)]
struct GithubRepositoryInfo {
    default_branch: String,
}

#[derive(Debug, Deserialize)]
struct GithubCommit {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GithubApiErrorResponse {
    message: Option<String>,
}

pub(super) struct GithubProvider {
    http_clients: Arc<HttpClientPool>,
}

impl GithubProvider {
    pub(super) fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
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

    async fn ensure_success_response(
        &self,
        response: Response,
        url: &Url,
    ) -> Result<Response, DomainError> {
        if response.status().is_success() {
            return Ok(response);
        }

        let error = read_provider_http_error(response).await;
        if let Some(domain_error) = classify_github_http_error(url, &error) {
            return Err(domain_error);
        }

        Err(provider_http_error_to_domain_error("GitHub", url, error))
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

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("GitHub request failed: {}", error))
            })?;

        let response = self.ensure_success_response(response, &url).await?;
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

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("GitHub request failed: {}", error))
            })?;

        let response = self.ensure_success_response(response, &url).await?;
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

        let http_client = self.http_clients.client(HttpClientProfile::Default)?;
        let response = http_client
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

        let response = self.ensure_success_response(response, &url).await?;
        parse_bytes_or_error(response, &url, "GitHub").await
    }
}

fn classify_github_http_error(url: &Url, error: &ProviderHttpError) -> Option<DomainError> {
    if !matches!(
        error.status,
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        return None;
    }

    let message = serde_json::from_str::<GithubApiErrorResponse>(&error.body)
        .ok()
        .and_then(|payload| payload.message)
        .unwrap_or_else(|| error.body.trim().to_string());
    let normalized = message.to_ascii_lowercase();
    if !GITHUB_RATE_LIMIT_TOKENS
        .iter()
        .any(|token| normalized.contains(token))
    {
        return None;
    }

    tracing::debug!(
        "GitHub API rate limit response for '{}': HTTP {} ({})",
        url,
        error.status,
        error.body.trim()
    );

    Some(DomainError::rate_limited(GITHUB_RATE_LIMIT_MESSAGE))
}

#[cfg(test)]
mod tests {
    use super::{GITHUB_RATE_LIMIT_MESSAGE, classify_github_http_error};
    use crate::domain::errors::DomainError;
    use crate::infrastructure::repositories::file_extension_repository::providers::ProviderHttpError;
    use reqwest::StatusCode;
    use url::Url;

    #[test]
    fn classifies_primary_github_rate_limit_as_domain_rate_limit() {
        let url = Url::parse("https://api.github.com/repos/owner/repo").expect("url");
        let error = ProviderHttpError {
            status: StatusCode::FORBIDDEN,
            body: r#"{"message":"API rate limit exceeded for 127.0.0.1."}"#.to_string(),
        };

        let classified = classify_github_http_error(&url, &error);

        assert!(matches!(
            classified,
            Some(DomainError::RateLimited {
                message
            }) if message == GITHUB_RATE_LIMIT_MESSAGE
        ));
    }

    #[test]
    fn ignores_non_rate_limit_github_responses() {
        let url = Url::parse("https://api.github.com/repos/owner/repo").expect("url");
        let error = ProviderHttpError {
            status: StatusCode::FORBIDDEN,
            body: r#"{"message":"Repository access blocked"}"#.to_string(),
        };

        assert!(classify_github_http_error(&url, &error).is_none());
    }

    #[test]
    fn classifies_github_abuse_detection_as_rate_limit() {
        let url = Url::parse("https://api.github.com/repos/owner/repo").expect("url");
        let error = ProviderHttpError {
            status: StatusCode::FORBIDDEN,
            body: r#"{"message":"You have triggered an abuse detection mechanism."}"#.to_string(),
        };

        assert!(matches!(
            classify_github_http_error(&url, &error),
            Some(DomainError::RateLimited { message }) if message == GITHUB_RATE_LIMIT_MESSAGE
        ));
    }
}
