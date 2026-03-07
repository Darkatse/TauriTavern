use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExtensionStoreScope {
    Local,
    Global,
}

impl ExtensionStoreScope {
    pub(super) fn from_global(global: bool) -> Self {
        if global { Self::Global } else { Self::Local }
    }

    pub(super) fn from_location(location: &str) -> Result<Self, DomainError> {
        match location {
            "local" => Ok(Self::Local),
            "global" => Ok(Self::Global),
            _ => Err(DomainError::InvalidData(format!(
                "Invalid extension location: {}",
                location
            ))),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(super) struct ExtensionSourceMetadata {
    pub(super) owner: String,
    pub(super) repo: String,
    pub(super) reference: String,
    pub(super) remote_url: String,
    pub(super) installed_commit: String,
}

pub(super) struct ExtensionSourceStore {
    root: PathBuf,
    local_root: PathBuf,
    global_root: PathBuf,
}

impl ExtensionSourceStore {
    pub(super) fn new(root: PathBuf) -> Self {
        let local_root = root.join("local");
        let global_root = root.join("global");
        Self {
            root,
            local_root,
            global_root,
        }
    }

    fn scope_root(&self, scope: ExtensionStoreScope) -> &Path {
        match scope {
            ExtensionStoreScope::Local => &self.local_root,
            ExtensionStoreScope::Global => &self.global_root,
        }
    }

    fn record_path(&self, scope: ExtensionStoreScope, extension_name: &str) -> PathBuf {
        self.scope_root(scope)
            .join(format!("{}.json", extension_name))
    }

    pub(super) fn legacy_record_path(extension_path: &Path) -> PathBuf {
        extension_path.join(SOURCE_METADATA_FILE)
    }

    pub(super) async fn read(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = self.record_path(scope, extension_name);
        if !path.exists() {
            return Ok(None);
        }

        let metadata: ExtensionSourceMetadata = read_json_file(&path).await?;
        Ok(Some(metadata))
    }

    pub(super) fn read_sync(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = self.record_path(scope, extension_name);
        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;
        let metadata =
            serde_json::from_str::<ExtensionSourceMetadata>(&contents).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Invalid extension source state '{}': {}",
                    path.display(),
                    error
                ))
            })?;

        Ok(Some(metadata))
    }

    pub(super) async fn write(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        metadata: &ExtensionSourceMetadata,
    ) -> Result<(), DomainError> {
        let path = self.record_path(scope, extension_name);
        let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        tokio_fs::write(&path, serialized).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }

    pub(super) fn write_sync(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        metadata: &ExtensionSourceMetadata,
    ) -> Result<(), DomainError> {
        let path = self.record_path(scope, extension_name);
        let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        fs::write(&path, serialized).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }

    pub(super) async fn delete(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<(), DomainError> {
        let path = self.record_path(scope, extension_name);
        if !path.exists() {
            return Ok(());
        }

        tokio_fs::remove_file(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }

    pub(super) async fn move_record(
        &self,
        source_scope: ExtensionStoreScope,
        destination_scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<(), DomainError> {
        let source_path = self.record_path(source_scope, extension_name);
        if !source_path.exists() {
            return Ok(());
        }

        let destination_path = self.record_path(destination_scope, extension_name);
        tokio_fs::rename(&source_path, &destination_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to move extension source state from '{}' to '{}': {}",
                    source_path.display(),
                    destination_path.display(),
                    error
                ))
            })
    }

    pub(super) fn migrate_all(
        &self,
        user_extensions_dir: &Path,
        global_extensions_dir: &Path,
    ) -> Result<(), DomainError> {
        self.migrate_scope(ExtensionStoreScope::Local, user_extensions_dir)?;
        self.migrate_scope(ExtensionStoreScope::Global, global_extensions_dir)?;
        Ok(())
    }

    fn migrate_scope(
        &self,
        scope: ExtensionStoreScope,
        extensions_dir: &Path,
    ) -> Result<(), DomainError> {
        if !extensions_dir.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(extensions_dir).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extensions directory '{}': {}",
                extensions_dir.display(),
                error
            ))
        })?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read extension directory entry in '{}': {}",
                    extensions_dir.display(),
                    error
                ))
            })?;

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let Some(file_name) = path.file_name() else {
                continue;
            };
            let extension_name = file_name.to_string_lossy().to_string();
            if extension_name.starts_with('.') {
                continue;
            }

            self.materialize_sync(scope, &extension_name, &path)?;
        }

        Ok(())
    }

    fn materialize_sync(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        extension_path: &Path,
    ) -> Result<(), DomainError> {
        if self.read_sync(scope, extension_name)?.is_some() {
            self.delete_legacy_sync(extension_path)?;
            return Ok(());
        }

        if let Some(metadata) = self.read_legacy_sync(extension_path)? {
            self.write_sync(scope, extension_name, &metadata)?;
            self.delete_legacy_sync(extension_path)?;
            return Ok(());
        }

        if let Some(metadata) = Self::infer_source_metadata_from_git_best_effort(extension_path) {
            self.write_sync(scope, extension_name, &metadata)?;
        }

        Ok(())
    }

    pub(super) async fn resolve_or_migrate(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        if let Some(metadata) = self.read(scope, extension_name).await? {
            return Ok(Some(metadata));
        }

        if let Some(metadata) = self.read_legacy(extension_path).await? {
            self.write(scope, extension_name, &metadata).await?;
            self.delete_legacy(extension_path).await?;
            return Ok(Some(metadata));
        }

        if let Some(metadata) = Self::infer_source_metadata_from_git_best_effort(extension_path) {
            self.write(scope, extension_name, &metadata).await?;
            return Ok(Some(metadata));
        }

        Ok(None)
    }

    async fn read_legacy(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(None);
        }

        let metadata: ExtensionSourceMetadata = read_json_file(&path).await?;
        Ok(Some(metadata))
    }

    fn read_legacy_sync(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read legacy extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;
        let metadata =
            serde_json::from_str::<ExtensionSourceMetadata>(&contents).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Invalid legacy extension source state '{}': {}",
                    path.display(),
                    error
                ))
            })?;

        Ok(Some(metadata))
    }

    async fn delete_legacy(&self, extension_path: &Path) -> Result<(), DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(());
        }

        tokio_fs::remove_file(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete legacy extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }

    fn delete_legacy_sync(&self, extension_path: &Path) -> Result<(), DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(());
        }

        fs::remove_file(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete legacy extension source state '{}': {}",
                path.display(),
                error
            ))
        })
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

            if let Some((key, value)) = trimmed.split_once('=')
                && key.trim() == "url"
            {
                return Some(value.trim().to_string());
            }
        }

        None
    }

    fn resolve_git_head_commit(
        git_dir: &Path,
        common_dir: Option<&Path>,
        head_content: &str,
    ) -> Option<String> {
        let trimmed = head_content.trim();
        if trimmed.is_empty() {
            return None;
        }

        fn read_ref_file_commit(root: &Path, ref_name: &str) -> Option<String> {
            let commit = fs::read_to_string(root.join(ref_name)).ok()?;
            let commit = commit.trim();
            if commit.is_empty() {
                None
            } else {
                Some(commit.to_string())
            }
        }

        fn read_packed_refs_commit(root: &Path, ref_name: &str) -> Option<String> {
            let packed_refs_path = root.join("packed-refs");
            let packed_refs = fs::read_to_string(packed_refs_path).ok()?;
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

            None
        }

        if let Some(reference) = trimmed.strip_prefix("ref: ") {
            let ref_name = reference.trim();
            if ref_name.is_empty() {
                return None;
            }

            if let Some(commit) = read_ref_file_commit(git_dir, ref_name) {
                return Some(commit);
            }

            if let Some(common_dir) = common_dir
                && let Some(commit) = read_ref_file_commit(common_dir, ref_name)
            {
                return Some(commit);
            }

            if let Some(commit) = read_packed_refs_commit(git_dir, ref_name) {
                return Some(commit);
            }

            if let Some(common_dir) = common_dir
                && let Some(commit) = read_packed_refs_commit(common_dir, ref_name)
            {
                return Some(commit);
            }

            return None;
        }

        Some(trimmed.to_string())
    }

    fn resolve_git_dir(extension_path: &Path) -> Result<Option<PathBuf>, DomainError> {
        let dot_git = extension_path.join(".git");
        if dot_git.is_dir() {
            return Ok(Some(dot_git));
        }

        if !dot_git.is_file() {
            return Ok(None);
        }

        let gitfile_contents = fs::read_to_string(&dot_git).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read gitdir file for '{}': {}",
                extension_path.display(),
                error
            ))
        })?;

        let first_line = gitfile_contents.lines().next().unwrap_or_default().trim();
        let Some(gitdir_value) = first_line.strip_prefix("gitdir:") else {
            return Ok(None);
        };
        let gitdir_value = gitdir_value.trim();
        if gitdir_value.is_empty() {
            return Ok(None);
        }

        let gitdir_path = PathBuf::from(gitdir_value);
        let gitdir_path = if gitdir_path.is_absolute() {
            gitdir_path
        } else {
            extension_path.join(gitdir_path)
        };

        if !gitdir_path.is_dir() {
            return Ok(None);
        }

        Ok(Some(gitdir_path))
    }

    fn resolve_git_common_dir(git_dir: &Path) -> Result<Option<PathBuf>, DomainError> {
        let commondir_path = git_dir.join("commondir");
        if !commondir_path.is_file() {
            return Ok(None);
        }

        let commondir_contents = fs::read_to_string(&commondir_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read git commondir at '{}': {}",
                commondir_path.display(),
                error
            ))
        })?;

        let commondir_value = commondir_contents.trim();
        if commondir_value.is_empty() {
            return Ok(None);
        }

        let commondir_path = PathBuf::from(commondir_value);
        let commondir_path = if commondir_path.is_absolute() {
            commondir_path
        } else {
            git_dir.join(commondir_path)
        };

        if !commondir_path.is_dir() {
            return Ok(None);
        }

        Ok(Some(commondir_path))
    }

    fn infer_source_metadata_from_git_best_effort(
        extension_path: &Path,
    ) -> Option<ExtensionSourceMetadata> {
        match Self::infer_source_metadata_from_git(extension_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::warn!(
                    "Failed to infer extension source metadata from git for '{}': {}",
                    extension_path.display(),
                    error
                );
                None
            }
        }
    }

    fn infer_source_metadata_from_git(
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let Some(git_dir) = Self::resolve_git_dir(extension_path)? else {
            return Ok(None);
        };

        let common_dir = Self::resolve_git_common_dir(&git_dir)?;

        let git_config_path = git_dir.join("config");
        let git_config = match fs::read_to_string(&git_config_path).or_else(|_| match &common_dir {
            Some(common_dir) => fs::read_to_string(common_dir.join("config")),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "git config not found",
            )),
        }) {
            Ok(config) => config,
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read git config for '{}': {}",
                    extension_path.display(),
                    error
                )));
            }
        };
        let Some(remote_url) = Self::parse_origin_remote_url(&git_config) else {
            return Ok(None);
        };

        let normalized_remote_url = Self::normalize_git_remote_url(&remote_url);
        let repo = parse_github_repo_url(&normalized_remote_url)?;

        let head_content = fs::read_to_string(git_dir.join("HEAD")).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read git HEAD for '{}': {}",
                extension_path.display(),
                error
            ))
        })?;
        let Some(installed_commit) =
            Self::resolve_git_head_commit(&git_dir, common_dir.as_deref(), &head_content)
        else {
            return Ok(None);
        };

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
            return Ok(None);
        }

        Ok(Some(ExtensionSourceMetadata {
            owner: repo.owner.clone(),
            repo: repo.repo.clone(),
            reference,
            remote_url: format!("https://github.com/{}/{}", repo.owner, repo.repo),
            installed_commit,
        }))
    }

    #[allow(dead_code)]
    pub(super) fn root(&self) -> &Path {
        &self.root
    }
}
