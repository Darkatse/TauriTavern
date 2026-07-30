use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use tt_domain::errors::DomainError;
use tt_domain::models::asset::{AssetCatalog, AssetCatalogEntry, AssetCategory, VrmAssetCatalog};
use tt_ports::repositories::asset_repository::AssetRepository;

pub struct FileAssetRepository {
    user_root: PathBuf,
    assets_dir: PathBuf,
    characters_dir: PathBuf,
}

impl FileAssetRepository {
    pub fn new(user_root: PathBuf, assets_dir: PathBuf, characters_dir: PathBuf) -> Self {
        Self {
            user_root,
            assets_dir,
            characters_dir,
        }
    }

    async fn ensure_asset_folders_exist(&self) -> Result<(), DomainError> {
        fs::create_dir_all(&self.assets_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create assets directory '{}': {}",
                    self.assets_dir.display(),
                    error
                ))
            })?;

        for category in AssetCategory::ALL {
            let path = self.assets_dir.join(category.as_str());
            match fs::metadata(&path).await {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return Err(DomainError::InvalidData(format!(
                        "Asset category path is not a directory: {}",
                        path.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir_all(&path).await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to create asset category directory '{}': {}",
                            path.display(),
                            error
                        ))
                    })?;
                }
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to inspect asset category directory '{}': {}",
                        path.display(),
                        error
                    )));
                }
            }
        }

        Ok(())
    }

    fn category_dir(&self, category: AssetCategory) -> PathBuf {
        self.assets_dir.join(category.as_str())
    }

    fn temp_file_path(&self, filename: &str) -> PathBuf {
        self.category_dir(AssetCategory::Temp).join(filename)
    }

    fn relative_from_user_root(&self, path: &Path) -> Result<String, DomainError> {
        path.strip_prefix(&self.user_root)
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .map_err(|_| {
                DomainError::InternalError(format!(
                    "Asset path is outside user root: {}",
                    path.display()
                ))
            })
    }

    async fn collect_files_recursive(root: &Path) -> Result<Vec<PathBuf>, DomainError> {
        match fs::metadata(root).await {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(DomainError::InvalidData(format!(
                    "Asset path is not a directory: {}",
                    root.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect asset directory '{}': {}",
                    root.display(),
                    error
                )));
            }
        }

        let mut files = Vec::new();
        let mut stack = vec![root.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let mut entries = fs::read_dir(&dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read asset directory '{}': {}",
                    dir.display(),
                    error
                ))
            })?;

            while let Some(entry) = entries.next_entry().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read asset directory entry '{}': {}",
                    dir.display(),
                    error
                ))
            })? {
                let file_type = entry.file_type().await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to inspect asset entry '{}': {}",
                        entry.path().display(),
                        error
                    ))
                })?;

                if file_type.is_dir() {
                    stack.push(entry.path());
                } else if file_type.is_file() {
                    files.push(entry.path());
                }
            }
        }

        files.sort();
        Ok(files)
    }

    async fn read_direct_file_names(root: &Path) -> Result<Vec<String>, DomainError> {
        match fs::metadata(root).await {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(DomainError::InvalidData(format!(
                    "Asset path is not a directory: {}",
                    root.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect asset directory '{}': {}",
                    root.display(),
                    error
                )));
            }
        }

        let mut names = Vec::new();
        let mut entries = fs::read_dir(root).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read asset directory '{}': {}",
                root.display(),
                error
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read asset directory entry '{}': {}",
                root.display(),
                error
            ))
        })? {
            let file_type = entry.file_type().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect asset entry '{}': {}",
                    entry.path().display(),
                    error
                ))
            })?;

            if !file_type.is_file() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if name != ".placeholder" {
                names.push(name);
            }
        }

        names.sort();
        Ok(names)
    }

    async fn list_live2d_assets(&self, root: &Path) -> Result<Vec<String>, DomainError> {
        let files = Self::collect_files_recursive(root).await?;
        let mut output = Vec::new();

        for file in files {
            let file_name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if file_name.contains("model") && file_name.ends_with(".json") {
                output.push(self.relative_from_user_root(&file)?);
            }
        }

        Ok(output)
    }

    async fn list_vrm_assets(&self, root: &Path) -> Result<VrmAssetCatalog, DomainError> {
        let model = self
            .list_recursive_relative_files(&root.join("model"), true)
            .await?;
        let animation = self
            .list_recursive_relative_files(&root.join("animation"), true)
            .await?;

        Ok(VrmAssetCatalog { model, animation })
    }

    async fn list_recursive_relative_files(
        &self,
        root: &Path,
        skip_placeholder: bool,
    ) -> Result<Vec<String>, DomainError> {
        let files = Self::collect_files_recursive(root).await?;
        let mut output = Vec::new();

        for file in files {
            if skip_placeholder
                && file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == ".placeholder")
            {
                continue;
            }

            output.push(self.relative_from_user_root(&file)?);
        }

        Ok(output)
    }
}

#[async_trait]
impl AssetRepository for FileAssetRepository {
    async fn list_assets(&self) -> Result<AssetCatalog, DomainError> {
        self.ensure_asset_folders_exist().await?;

        let mut catalog = AssetCatalog::new();
        for category in AssetCategory::ALL {
            if category.is_temp() {
                continue;
            }

            let category_dir = self.category_dir(category);
            let entry = match category {
                AssetCategory::Live2d => {
                    AssetCatalogEntry::Files(self.list_live2d_assets(&category_dir).await?)
                }
                AssetCategory::Vrm => {
                    AssetCatalogEntry::Vrm(self.list_vrm_assets(&category_dir).await?)
                }
                AssetCategory::Bgm
                | AssetCategory::Ambient
                | AssetCategory::Blip
                | AssetCategory::Character => {
                    let files = Self::read_direct_file_names(&category_dir)
                        .await?
                        .into_iter()
                        .map(|file| format!("assets/{}/{}", category.as_str(), file))
                        .collect();
                    AssetCatalogEntry::Files(files)
                }
                AssetCategory::Temp => unreachable!("temp category is skipped"),
            };

            catalog.insert(category.as_str().to_string(), entry);
        }

        Ok(catalog)
    }

    async fn stage_asset_file(&self, filename: &str) -> Result<PathBuf, DomainError> {
        self.ensure_asset_folders_exist().await?;

        let temp_path = self.temp_file_path(filename);
        match fs::remove_file(&temp_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to remove stale asset temp file '{}': {}",
                    temp_path.display(),
                    error
                )));
            }
        }

        Ok(temp_path)
    }

    async fn commit_staged_asset_file(
        &self,
        category: AssetCategory,
        filename: &str,
    ) -> Result<(), DomainError> {
        let temp_path = self.temp_file_path(filename);
        let target_path = self.category_dir(category).join(filename);
        if target_path == temp_path {
            return Ok(());
        }

        fs::copy(&temp_path, &target_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to move asset file '{}' to '{}': {}",
                temp_path.display(),
                target_path.display(),
                error
            ))
        })?;

        fs::remove_file(&temp_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to remove asset temp file '{}': {}",
                temp_path.display(),
                error
            ))
        })?;

        Ok(())
    }

    async fn discard_staged_asset_file(&self, filename: &str) -> Result<(), DomainError> {
        let temp_path = self.temp_file_path(filename);
        match fs::remove_file(&temp_path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DomainError::InternalError(format!(
                "Failed to remove asset temp file '{}': {}",
                temp_path.display(),
                error
            ))),
        }
    }

    async fn delete_asset_file(
        &self,
        category: AssetCategory,
        filename: &str,
    ) -> Result<(), DomainError> {
        let target_path = self.category_dir(category).join(filename);
        match fs::metadata(&target_path).await {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                return Err(DomainError::InvalidData(format!(
                    "Asset is not a file: {}",
                    target_path.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(DomainError::InvalidData("Asset not found.".to_string()));
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect asset file '{}': {}",
                    target_path.display(),
                    error
                )));
            }
        }

        fs::remove_file(&target_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete asset file '{}': {}",
                target_path.display(),
                error
            ))
        })
    }

    async fn list_character_assets(
        &self,
        character_name: &str,
        category: AssetCategory,
    ) -> Result<Vec<String>, DomainError> {
        let folder = self
            .characters_dir
            .join(character_name)
            .join(category.as_str());
        if category == AssetCategory::Live2d {
            let files = Self::collect_files_recursive(&folder).await?;
            let mut output = Vec::new();
            for file in files {
                let file_name = file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                if file_name.contains("model") && file_name.ends_with(".json") {
                    output.push(self.relative_from_user_root(&file)?);
                }
            }
            return Ok(output);
        }

        let files = Self::read_direct_file_names(&folder).await?;
        Ok(files
            .into_iter()
            .map(|file| {
                format!(
                    "/characters/{}/{}/{}",
                    character_name,
                    category.as_str(),
                    file
                )
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::fs as std_fs;
    use std::path::{Path, PathBuf};

    use uuid::Uuid;

    use tt_domain::models::asset::{AssetCatalogEntry, AssetCategory};
    use tt_ports::repositories::asset_repository::AssetRepository;

    use super::FileAssetRepository;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "tauritavern-asset-repository-{}-{}",
                label,
                Uuid::new_v4()
            ));
            std_fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std_fs::remove_dir_all(&self.path);
        }
    }

    fn repository(root: &Path) -> FileAssetRepository {
        let user_root = root.join("default-user");
        FileAssetRepository::new(
            user_root.clone(),
            user_root.join("assets"),
            user_root.join("characters"),
        )
    }

    #[tokio::test]
    async fn saves_lists_and_deletes_audio_assets() {
        let temp = TempDirGuard::new("audio");
        let repository = repository(temp.path());

        let temp_path = repository.stage_asset_file("theme.mp3").await.unwrap();
        tokio::fs::write(&temp_path, b"audio").await.unwrap();
        repository
            .commit_staged_asset_file(AssetCategory::Bgm, "theme.mp3")
            .await
            .unwrap();

        let catalog = repository.list_assets().await.unwrap();
        assert_eq!(
            catalog.get("bgm"),
            Some(&AssetCatalogEntry::Files(vec![
                "assets/bgm/theme.mp3".to_string()
            ]))
        );
        assert!(
            !temp
                .path()
                .join("default-user/assets/temp/theme.mp3")
                .exists()
        );

        repository
            .delete_asset_file(AssetCategory::Bgm, "theme.mp3")
            .await
            .unwrap();

        let catalog = repository.list_assets().await.unwrap();
        assert_eq!(
            catalog.get("bgm"),
            Some(&AssetCatalogEntry::Files(Vec::new()))
        );
    }

    #[tokio::test]
    async fn stages_and_commits_downloaded_assets_without_leaving_temp_file() {
        let temp = TempDirGuard::new("staged");
        let repository = repository(temp.path());

        let temp_path = repository.stage_asset_file("theme.mp3").await.unwrap();
        tokio::fs::write(&temp_path, b"audio").await.unwrap();
        repository
            .commit_staged_asset_file(AssetCategory::Bgm, "theme.mp3")
            .await
            .unwrap();

        let user_root = temp.path().join("default-user");
        assert!(!user_root.join("assets/temp/theme.mp3").exists());
        assert_eq!(
            std_fs::read(user_root.join("assets/bgm/theme.mp3")).unwrap(),
            b"audio"
        );
    }

    #[tokio::test]
    async fn discards_staged_asset_downloads() {
        let temp = TempDirGuard::new("discard");
        let repository = repository(temp.path());

        let temp_path = repository.stage_asset_file("theme.mp3").await.unwrap();
        tokio::fs::write(&temp_path, b"partial").await.unwrap();
        repository
            .discard_staged_asset_file("theme.mp3")
            .await
            .unwrap();

        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn lists_live2d_and_vrm_assets_with_client_relative_paths() {
        let temp = TempDirGuard::new("special");
        let user_root = temp.path().join("default-user");
        std_fs::create_dir_all(user_root.join("assets/live2d/model-a")).unwrap();
        std_fs::write(
            user_root.join("assets/live2d/model-a/avatar.model3.json"),
            "{}",
        )
        .unwrap();
        std_fs::write(user_root.join("assets/live2d/model-a/readme.txt"), "").unwrap();
        std_fs::create_dir_all(user_root.join("assets/vrm/model")).unwrap();
        std_fs::create_dir_all(user_root.join("assets/vrm/animation")).unwrap();
        std_fs::write(user_root.join("assets/vrm/model/body.vrm"), "").unwrap();
        std_fs::write(user_root.join("assets/vrm/animation/wave.vrma"), "").unwrap();

        let catalog = repository(temp.path()).list_assets().await.unwrap();

        assert_eq!(
            catalog.get("live2d"),
            Some(&AssetCatalogEntry::Files(vec![
                "assets/live2d/model-a/avatar.model3.json".to_string()
            ]))
        );

        match catalog.get("vrm").unwrap() {
            AssetCatalogEntry::Vrm(vrm) => {
                assert_eq!(vrm.model, vec!["assets/vrm/model/body.vrm"]);
                assert_eq!(vrm.animation, vec!["assets/vrm/animation/wave.vrma"]);
            }
            AssetCatalogEntry::Files(_) => panic!("expected vrm catalog"),
        }
    }

    #[tokio::test]
    async fn lists_character_assets() {
        let temp = TempDirGuard::new("character");
        let user_root = temp.path().join("default-user");
        std_fs::create_dir_all(user_root.join("characters/Alice/bgm")).unwrap();
        std_fs::write(user_root.join("characters/Alice/bgm/theme.mp3"), "").unwrap();
        std_fs::write(user_root.join("characters/Alice/bgm/.placeholder"), "").unwrap();
        std_fs::create_dir_all(user_root.join("characters/Alice/live2d/model-a")).unwrap();
        std_fs::write(
            user_root.join("characters/Alice/live2d/model-a/avatar.model3.json"),
            "{}",
        )
        .unwrap();

        let repository = repository(temp.path());

        assert_eq!(
            repository
                .list_character_assets("Alice", AssetCategory::Bgm)
                .await
                .unwrap(),
            vec!["/characters/Alice/bgm/theme.mp3"]
        );
        assert_eq!(
            repository
                .list_character_assets("Alice", AssetCategory::Live2d)
                .await
                .unwrap(),
            vec!["characters/Alice/live2d/model-a/avatar.model3.json"]
        );
    }
}
