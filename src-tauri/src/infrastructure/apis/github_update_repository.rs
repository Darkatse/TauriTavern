use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::domain::errors::DomainError;
use crate::domain::models::update::ReleaseInfo;
use crate::domain::repositories::update_repository::UpdateRepository;
use crate::infrastructure::http_client::build_http_client;

const GITHUB_API_LATEST_RELEASE: &str =
    "https://api.github.com/repos/Darkatse/TauriTavern/releases/latest";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    prerelease: bool,
    published_at: Option<String>,
}

pub struct GitHubUpdateRepository {
    client: Client,
}

impl GitHubUpdateRepository {
    pub fn new() -> Result<Self, DomainError> {
        let client = build_http_client(Client::builder()).map_err(|error| {
            DomainError::InternalError(format!("Failed to build HTTP client: {error}"))
        })?;

        Ok(Self { client })
    }
}

#[async_trait]
impl UpdateRepository for GitHubUpdateRepository {
    async fn get_latest_release(&self) -> Result<ReleaseInfo, DomainError> {
        let response: GitHubRelease = self
            .client
            .get(GITHUB_API_LATEST_RELEASE)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("GitHub API request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| DomainError::InternalError(format!("GitHub API error: {error}")))?
            .json()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Failed to parse GitHub response: {error}"))
            })?;

        let version = parse_version_from_tag(&response.tag_name);

        Ok(ReleaseInfo {
            tag_name: response.tag_name,
            version,
            name: response.name.unwrap_or_default(),
            body: response.body.unwrap_or_default(),
            html_url: response.html_url,
            prerelease: response.prerelease,
            published_at: response.published_at.unwrap_or_default(),
        })
    }
}

fn parse_version_from_tag(tag: &str) -> String {
    let tag = tag.trim();
    let Some(start) = tag.find(|c: char| c.is_ascii_digit()) else {
        return tag.to_string();
    };

    let candidate = &tag[start..];
    let end = candidate
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(candidate.len());
    candidate[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_version_from_tag;

    #[test]
    fn desktop_auto_tag() {
        assert_eq!(parse_version_from_tag("desktop-auto-v1.4.0"), "1.4.0");
    }

    #[test]
    fn simple_v_tag() {
        assert_eq!(parse_version_from_tag("v1.4.0"), "1.4.0");
    }

    #[test]
    fn bare_version() {
        assert_eq!(parse_version_from_tag("1.4.0"), "1.4.0");
    }

    #[test]
    fn mobile_tag() {
        assert_eq!(parse_version_from_tag("mobile-v2.0.0"), "2.0.0");
    }

    #[test]
    fn suffix_is_stripped() {
        assert_eq!(parse_version_from_tag("v1.4.0-beta.1"), "1.4.0");
    }
}

