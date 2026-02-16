use super::*;

impl FileExtensionRepository {
    pub(super) async fn read_source_metadata(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let metadata_path = Self::source_metadata_path(extension_path);
        if !metadata_path.exists() {
            return Ok(None);
        }

        let metadata: ExtensionSourceMetadata = read_json_file(&metadata_path).await?;
        Ok(Some(metadata))
    }

    pub(super) async fn write_source_metadata(
        &self,
        extension_path: &Path,
        metadata: &ExtensionSourceMetadata,
    ) -> Result<(), DomainError> {
        let metadata_path = Self::source_metadata_path(extension_path);
        let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize extension source metadata for '{}': {}",
                extension_path.display(),
                error
            ))
        })?;

        tokio_fs::write(&metadata_path, serialized)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write extension source metadata '{}': {}",
                    metadata_path.display(),
                    error
                ))
            })
    }

    fn parse_origin_remote_url(config: &str) -> Option<String> {
        let mut in_origin = false;

        for line in config.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_origin = trimmed == "[remote \"origin\"]";
                continue;
            }

            if !in_origin {
                continue;
            }

            if let Some((key, value)) = trimmed.split_once('=') {
                if key.trim() == "url" {
                    return Some(value.trim().to_string());
                }
            }
        }

        None
    }

    fn normalize_git_remote_url(remote_url: &str) -> String {
        let trimmed = remote_url.trim();

        if let Some(path) = trimmed.strip_prefix("git@github.com:") {
            return format!("https://github.com/{}", path);
        }

        if let Some(path) = trimmed.strip_prefix("ssh://git@github.com/") {
            return format!("https://github.com/{}", path);
        }

        trimmed.to_string()
    }

    fn resolve_git_head_commit(git_dir: &Path, head_content: &str) -> Option<String> {
        let trimmed = head_content.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Some(reference) = trimmed.strip_prefix("ref: ") {
            let ref_name = reference.trim();
            if ref_name.is_empty() {
                return None;
            }

            let ref_path = git_dir.join(ref_name);
            if let Ok(commit) = fs::read_to_string(ref_path) {
                let commit = commit.trim();
                if !commit.is_empty() {
                    return Some(commit.to_string());
                }
            }

            let packed_refs_path = git_dir.join("packed-refs");
            if let Ok(packed_refs) = fs::read_to_string(packed_refs_path) {
                for line in packed_refs.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                        continue;
                    }

                    let mut parts = line.split_whitespace();
                    let Some(commit) = parts.next() else {
                        continue;
                    };
                    let Some(name) = parts.next() else {
                        continue;
                    };

                    if name == ref_name {
                        return Some(commit.to_string());
                    }
                }
            }

            return None;
        }

        Some(trimmed.to_string())
    }

    fn infer_source_metadata_from_git(
        &self,
        extension_path: &Path,
    ) -> Option<ExtensionSourceMetadata> {
        let git_dir = extension_path.join(".git");
        if !git_dir.is_dir() {
            return None;
        }

        let git_config = fs::read_to_string(git_dir.join("config")).ok()?;
        let remote_url = Self::parse_origin_remote_url(&git_config)?;
        let normalized_remote_url = Self::normalize_git_remote_url(&remote_url);
        let repo = self.parse_github_repo_url(&normalized_remote_url).ok()?;

        let head_content = fs::read_to_string(git_dir.join("HEAD")).ok()?;
        let installed_commit = Self::resolve_git_head_commit(&git_dir, &head_content)?;
        let reference = if let Some(head_ref) = head_content.trim().strip_prefix("ref: ") {
            head_ref
                .trim()
                .strip_prefix("refs/heads/")
                .unwrap_or(head_ref.trim())
                .to_string()
        } else {
            installed_commit.clone()
        };

        if reference.trim().is_empty() {
            return None;
        }

        Some(ExtensionSourceMetadata {
            owner: repo.owner.clone(),
            repo: repo.repo.clone(),
            reference,
            remote_url: format!("https://github.com/{}/{}", repo.owner, repo.repo),
            installed_commit,
        })
    }

    pub(super) async fn resolve_source_metadata(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        if let Some(metadata) = self.read_source_metadata(extension_path).await? {
            return Ok(Some(metadata));
        }

        Ok(self.infer_source_metadata_from_git(extension_path))
    }
}
