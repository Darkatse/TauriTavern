use super::*;

impl FileExtensionRepository {
    pub(super) fn parse_github_repo_url(
        &self,
        url: &str,
    ) -> Result<GithubRepoLocation, DomainError> {
        let parsed_url = Url::parse(url).map_err(|error| {
            DomainError::InvalidData(format!("Invalid GitHub URL '{}': {}", url, error))
        })?;

        let host = parsed_url
            .host_str()
            .unwrap_or_default()
            .to_ascii_lowercase();

        if host != "github.com" && host != "www.github.com" {
            return Err(DomainError::InvalidData(
                "Only GitHub repositories are supported".to_string(),
            ));
        }

        let path_segments = parsed_url
            .path_segments()
            .ok_or_else(|| DomainError::InvalidData("Invalid GitHub URL path".to_string()))?
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<String>>();

        if path_segments.len() < 2 {
            return Err(DomainError::InvalidData(
                "GitHub URL must include owner and repository".to_string(),
            ));
        }

        let owner = path_segments[0].trim().to_string();
        let repo = path_segments[1].trim_end_matches(".git").trim().to_string();

        if owner.is_empty() || repo.is_empty() {
            return Err(DomainError::InvalidData(
                "GitHub owner/repository cannot be empty".to_string(),
            ));
        }

        let reference_from_url = if path_segments.len() >= 4 && path_segments[2] == "tree" {
            let reference = path_segments[3..].join("/");
            if reference.is_empty() {
                None
            } else {
                Some(reference)
            }
        } else {
            parsed_url
                .query_pairs()
                .find(|(key, _)| key == "ref")
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };

        Ok(GithubRepoLocation {
            owner,
            repo,
            reference_from_url,
        })
    }

    pub(super) fn normalize_requested_reference(reference: Option<String>) -> Option<String> {
        reference
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    pub(super) fn build_github_api_url(&self, segments: &[&str]) -> Result<Url, DomainError> {
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

    pub(super) async fn github_get_json<T>(&self, segments: &[&str]) -> Result<T, DomainError>
    where
        T: DeserializeOwned,
    {
        let url = self.build_github_api_url(segments)?;

        let response = self
            .http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("GitHub request failed: {}", error))
            })?;

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
                "GitHub request failed for '{}': HTTP {}{}",
                url, status, suffix
            )));
        }

        response.json::<T>().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to parse GitHub response for '{}': {}",
                url, error
            ))
        })
    }

    pub(super) async fn github_get_default_branch(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<String, DomainError> {
        let info: GithubRepositoryInfo = self.github_get_json(&["repos", owner, repo]).await?;
        if info.default_branch.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}/{}' has no default branch",
                owner, repo
            )));
        }

        Ok(info.default_branch)
    }

    pub(super) async fn github_get_latest_commit_hash(
        &self,
        owner: &str,
        repo: &str,
        reference: &str,
    ) -> Result<String, DomainError> {
        let commit: GithubCommit = self
            .github_get_json(&["repos", owner, repo, "commits", reference])
            .await?;

        if commit.sha.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}/{}' returned an empty commit SHA for reference '{}'",
                owner, repo, reference
            )));
        }

        Ok(commit.sha)
    }
}
