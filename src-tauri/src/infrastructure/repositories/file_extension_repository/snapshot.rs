use super::*;

impl FileExtensionRepository {
    pub(super) async fn create_temp_directory(
        &self,
        parent: &Path,
        prefix: &str,
    ) -> Result<PathBuf, DomainError> {
        for _ in 0..8 {
            let candidate = parent.join(format!(".{}-{}", prefix, Uuid::new_v4()));
            if !candidate.exists() {
                tokio_fs::create_dir_all(&candidate)
                    .await
                    .map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to create temporary directory '{}': {}",
                            candidate.display(),
                            error
                        ))
                    })?;
                return Ok(candidate);
            }
        }

        Err(DomainError::InternalError(
            "Failed to allocate temporary directory for extension operation".to_string(),
        ))
    }

    pub(super) async fn cleanup_temp_directory(path: &Path) {
        if path.exists() {
            let _ = tokio_fs::remove_dir_all(path).await;
        }
    }

    fn strip_archive_root(path: &Path) -> Option<PathBuf> {
        let mut components = path.components();
        components.next()?;
        let remainder = components.as_path();

        if remainder.as_os_str().is_empty() {
            None
        } else {
            Some(remainder.to_path_buf())
        }
    }

    fn extract_zip_bytes(&self, bytes: &[u8], destination: &Path) -> Result<(), DomainError> {
        let reader = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).map_err(|error| {
            DomainError::InternalError(format!("Failed to read downloaded ZIP archive: {}", error))
        })?;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                DomainError::InternalError(format!("Failed to read ZIP entry: {}", error))
            })?;

            // Skip entries that are not safely enclosed paths.
            let enclosed_path = match entry.enclosed_name() {
                Some(path) => path.to_path_buf(),
                None => continue,
            };

            // GitHub archives always wrap files in a top-level root folder.
            let relative_path = match Self::strip_archive_root(&enclosed_path) {
                Some(path) => path,
                None => continue,
            };

            let output_path = destination.join(relative_path);

            if entry.is_dir() {
                fs::create_dir_all(&output_path).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create directory '{}': {}",
                        output_path.display(),
                        error
                    ))
                })?;
                continue;
            }

            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create directory '{}': {}",
                        parent.display(),
                        error
                    ))
                })?;
            }

            let mut output_file = fs::File::create(&output_path).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create file '{}': {}",
                    output_path.display(),
                    error
                ))
            })?;

            std::io::copy(&mut entry, &mut output_file).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write file '{}': {}",
                    output_path.display(),
                    error
                ))
            })?;
        }

        Ok(())
    }

    pub(super) async fn download_and_extract_snapshot(
        &self,
        owner: &str,
        repo: &str,
        commit_hash: &str,
        destination: &Path,
    ) -> Result<(), DomainError> {
        let url = self.build_github_api_url(&["repos", owner, repo, "zipball", commit_hash])?;

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
                "Failed to download extension archive from '{}': HTTP {}{}",
                url, status, suffix
            )));
        }

        let archive_bytes = response.bytes().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extension archive response: {}",
                error
            ))
        })?;

        self.extract_zip_bytes(archive_bytes.as_ref(), destination)
    }

    pub(super) async fn required_manifest(
        &self,
        extension_path: &Path,
    ) -> Result<ExtensionManifest, DomainError> {
        match self.get_manifest(extension_path).await? {
            Some(manifest) => Ok(manifest),
            None => Err(DomainError::InvalidData(
                "Extension manifest not found".to_string(),
            )),
        }
    }

    pub(super) fn short_commit_hash(commit_hash: &str) -> String {
        commit_hash.chars().take(7).collect()
    }

    pub(super) fn replace_directory(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), DomainError> {
        let destination_name = destination
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "extension".to_string());
        let backup_path =
            destination.with_file_name(format!(".backup-{}-{}", destination_name, Uuid::new_v4()));

        fs::rename(destination, &backup_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to move existing extension '{}' to temporary backup '{}': {}",
                destination.display(),
                backup_path.display(),
                error
            ))
        })?;

        if let Err(error) = fs::rename(source, destination) {
            let _ = fs::rename(&backup_path, destination);
            return Err(DomainError::InternalError(format!(
                "Failed to activate updated extension '{}': {}",
                destination.display(),
                error
            )));
        }

        if let Err(error) = fs::remove_dir_all(&backup_path) {
            logger::warn(&format!(
                "Failed to remove extension backup directory '{}': {}",
                backup_path.display(),
                error
            ));
        }

        Ok(())
    }

    pub(super) fn resolve_move_dir<'a>(&'a self, location: &str) -> Result<&'a Path, DomainError> {
        match location {
            "global" => Ok(&self.global_extensions_dir),
            "local" => Ok(&self.user_extensions_dir),
            _ => Err(DomainError::InvalidData(format!(
                "Invalid extension location: {}",
                location
            ))),
        }
    }
}

pub(super) fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let path = entry.path();
        let file_name = path.file_name().unwrap();
        let target = dst.join(file_name);

        if ty.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
