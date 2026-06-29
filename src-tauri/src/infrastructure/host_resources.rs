use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use crate::application::services::host_resource_service::ports::{
    HostResourceAssetStore, HostResourceBinaryAsset, HostResourceFileStat, HostResourceStoreError,
    ThumbnailAssetRequest, ThumbnailKind,
};
use crate::application::services::host_resource_service::range::ByteRange;
use crate::application::services::host_resource_service::routes::UserDataAssetKind;
use crate::domain::errors::DomainError;
use crate::domain::models::user_directory::UserDirectory;
use crate::infrastructure::persistence::thumbnail_cache::{
    ThumbnailConfig, read_thumbnail_or_original_sync,
};
use crate::infrastructure::thumbnails::{avatar_thumbnail_config, background_thumbnail_config};

#[derive(Debug, Clone)]
struct HostResourceRoots {
    user_css_file: PathBuf,
    local_extensions_dir: PathBuf,
    global_extensions_dir: PathBuf,
    characters_dir: PathBuf,
    avatars_dir: PathBuf,
    backgrounds_dir: PathBuf,
    assets_dir: PathBuf,
    user_images_dir: PathBuf,
    user_files_dir: PathBuf,
    thumbnails_bg_dir: PathBuf,
    thumbnails_avatar_dir: PathBuf,
    thumbnails_persona_dir: PathBuf,
}

impl HostResourceRoots {
    fn from_data_root(data_root: impl AsRef<Path>) -> Self {
        let data_root = data_root.as_ref();
        let user_dirs = UserDirectory::default_user(data_root);

        Self {
            user_css_file: data_root.join("_css").join("user.css"),
            local_extensions_dir: data_root.join("default-user").join("extensions"),
            global_extensions_dir: data_root.join("extensions").join("third-party"),
            characters_dir: user_dirs.characters,
            avatars_dir: user_dirs.avatars,
            backgrounds_dir: user_dirs.backgrounds,
            assets_dir: user_dirs.assets,
            user_images_dir: user_dirs.user_images,
            user_files_dir: user_dirs.files,
            thumbnails_bg_dir: user_dirs.thumbnails_bg,
            thumbnails_avatar_dir: user_dirs.thumbnails_avatar,
            thumbnails_persona_dir: user_dirs.thumbnails_persona,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FilesystemHostResourceStore {
    roots: HostResourceRoots,
}

struct ResolvedHostResourceFile {
    stat: HostResourceFileStat,
}

struct OpenHostResourceFile {
    path: PathBuf,
    file: fs::File,
    stat: HostResourceFileStat,
}

impl FilesystemHostResourceStore {
    pub(crate) fn from_data_root(data_root: impl AsRef<Path>) -> Self {
        Self {
            roots: HostResourceRoots::from_data_root(data_root),
        }
    }

    #[cfg(test)]
    fn new(roots: HostResourceRoots) -> Self {
        Self { roots }
    }

    fn user_data_root(&self, kind: UserDataAssetKind) -> &Path {
        match kind {
            UserDataAssetKind::Character => &self.roots.characters_dir,
            UserDataAssetKind::Persona => &self.roots.avatars_dir,
            UserDataAssetKind::Background => &self.roots.backgrounds_dir,
            UserDataAssetKind::Asset => &self.roots.assets_dir,
            UserDataAssetKind::UserImage => &self.roots.user_images_dir,
            UserDataAssetKind::UserFile => &self.roots.user_files_dir,
        }
    }

    fn thumbnail_paths(&self, request: &ThumbnailAssetRequest) -> (&Path, &Path, ThumbnailConfig) {
        match request.kind {
            ThumbnailKind::Avatar => (
                &self.roots.characters_dir,
                &self.roots.thumbnails_avatar_dir,
                avatar_thumbnail_config(),
            ),
            ThumbnailKind::Persona => (
                &self.roots.avatars_dir,
                &self.roots.thumbnails_persona_dir,
                avatar_thumbnail_config(),
            ),
            ThumbnailKind::Background => (
                &self.roots.backgrounds_dir,
                &self.roots.thumbnails_bg_dir,
                background_thumbnail_config(),
            ),
        }
    }

    fn resolve_third_party_asset(
        &self,
        extension_folder: &str,
        relative_path: &Path,
    ) -> Result<ResolvedHostResourceFile, HostResourceStoreError> {
        for root in [
            &self.roots.local_extensions_dir,
            &self.roots.global_extensions_dir,
        ] {
            let path = root.join(extension_folder).join(relative_path);
            match stat_file(&path) {
                Ok(stat) => return Ok(ResolvedHostResourceFile { stat }),
                Err(HostResourceStoreError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        Err(HostResourceStoreError::not_found(format!(
            "Third-party extension asset not found: {}/{}",
            extension_folder,
            relative_path.display()
        )))
    }

    fn open_third_party_asset(
        &self,
        extension_folder: &str,
        relative_path: &Path,
    ) -> Result<OpenHostResourceFile, HostResourceStoreError> {
        for root in [
            &self.roots.local_extensions_dir,
            &self.roots.global_extensions_dir,
        ] {
            let path = root.join(extension_folder).join(relative_path);
            match open_file(&path) {
                Ok(opened) => return Ok(opened),
                Err(HostResourceStoreError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        Err(HostResourceStoreError::not_found(format!(
            "Third-party extension asset not found: {}/{}",
            extension_folder,
            relative_path.display()
        )))
    }
}

impl HostResourceAssetStore for FilesystemHostResourceStore {
    fn read_user_css(&self) -> Result<Vec<u8>, HostResourceStoreError> {
        read_file(&self.roots.user_css_file)
    }

    fn stat_third_party_asset(
        &self,
        extension_folder: &str,
        relative_path: &Path,
    ) -> Result<HostResourceFileStat, HostResourceStoreError> {
        self.resolve_third_party_asset(extension_folder, relative_path)
            .map(|resolved| resolved.stat)
    }

    fn read_third_party_asset(
        &self,
        extension_folder: &str,
        relative_path: &Path,
        max_len: Option<u64>,
    ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
        let opened = self.open_third_party_asset(extension_folder, relative_path)?;

        if let Some(limit_bytes) = max_len {
            if opened.stat.len > limit_bytes {
                return Err(HostResourceStoreError::payload_too_large(
                    opened.stat.len,
                    limit_bytes,
                ));
            }
        }

        let bytes = read_open_file(opened.file, &opened.path, max_len)?;

        Ok(HostResourceBinaryAsset {
            bytes,
            mime_type: opened.stat.mime_type,
        })
    }

    fn stat_user_data_asset(
        &self,
        kind: UserDataAssetKind,
        relative_path: &Path,
    ) -> Result<HostResourceFileStat, HostResourceStoreError> {
        stat_scoped_file(self.user_data_root(kind), relative_path)
    }

    fn read_user_data_asset(
        &self,
        kind: UserDataAssetKind,
        relative_path: &Path,
    ) -> Result<Vec<u8>, HostResourceStoreError> {
        read_scoped_file(self.user_data_root(kind), relative_path)
    }

    fn read_user_data_asset_range(
        &self,
        kind: UserDataAssetKind,
        relative_path: &Path,
        range: ByteRange,
    ) -> Result<Vec<u8>, HostResourceStoreError> {
        read_scoped_file_range(self.user_data_root(kind), relative_path, range)
    }

    fn read_thumbnail_asset(
        &self,
        request: ThumbnailAssetRequest,
    ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
        let (original_root, thumbnail_root, config) = self.thumbnail_paths(&request);
        let original_path = original_root.join(&request.file);
        let thumbnail_path = thumbnail_root.join(&request.file);

        stat_file(&original_path)?;

        if !request.use_thumbnails {
            return read_binary_asset(&original_path);
        }

        let asset = read_thumbnail_or_original_sync(&original_path, &thumbnail_path, config)
            .map_err(host_resource_error_from_domain)?;

        Ok(HostResourceBinaryAsset {
            bytes: asset.bytes,
            mime_type: asset.mime_type,
        })
    }
}

fn stat_scoped_file(
    root: &Path,
    relative_path: &Path,
) -> Result<HostResourceFileStat, HostResourceStoreError> {
    stat_file(&root.join(relative_path))
}

fn read_scoped_file(root: &Path, relative_path: &Path) -> Result<Vec<u8>, HostResourceStoreError> {
    read_file(&root.join(relative_path))
}

fn read_scoped_file_range(
    root: &Path,
    relative_path: &Path,
    range: ByteRange,
) -> Result<Vec<u8>, HostResourceStoreError> {
    let path = root.join(relative_path);
    stat_file(&path)?;

    let range_len = usize::try_from(range.len())
        .map_err(|_| HostResourceStoreError::internal("Range is too large to serve"))?;
    let mut file = fs::File::open(&path).map_err(|error| io_error(&path, error, "open"))?;
    file.seek(std::io::SeekFrom::Start(range.start))
        .map_err(|error| {
            HostResourceStoreError::internal(format!(
                "Failed to seek host resource '{}': {}",
                path.display(),
                error
            ))
        })?;

    let mut bytes = vec![0u8; range_len];
    file.read_exact(&mut bytes).map_err(|error| {
        HostResourceStoreError::internal(format!(
            "Failed to read host resource range '{}': {}",
            path.display(),
            error
        ))
    })?;

    Ok(bytes)
}

fn read_binary_asset(path: &Path) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
    let bytes = read_file(path)?;
    Ok(HostResourceBinaryAsset {
        bytes,
        mime_type: mime_type_for_path(path),
    })
}

fn read_file(path: &Path) -> Result<Vec<u8>, HostResourceStoreError> {
    stat_file(path)?;
    fs::read(path).map_err(|error| io_error(path, error, "read"))
}

fn open_file(path: &Path) -> Result<OpenHostResourceFile, HostResourceStoreError> {
    stat_file(path)?;
    let file = fs::File::open(path).map_err(|error| io_error(path, error, "open"))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error(path, error, "stat"))?;

    if !metadata.is_file() {
        return Err(HostResourceStoreError::not_found(format!(
            "Host resource not found: {}",
            path.display()
        )));
    }

    Ok(OpenHostResourceFile {
        path: path.to_path_buf(),
        file,
        stat: HostResourceFileStat {
            len: metadata.len(),
            mime_type: mime_type_for_path(path),
        },
    })
}

fn read_open_file(
    file: fs::File,
    path: &Path,
    max_len: Option<u64>,
) -> Result<Vec<u8>, HostResourceStoreError> {
    let mut bytes = Vec::new();

    if let Some(limit_bytes) = max_len {
        let mut limited = file.take(limit_bytes.saturating_add(1));
        limited
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(path, error, "read"))?;

        if bytes.len() as u64 > limit_bytes {
            return Err(HostResourceStoreError::payload_too_large(
                bytes.len() as u64,
                limit_bytes,
            ));
        }

        return Ok(bytes);
    }

    let mut file = file;
    file.read_to_end(&mut bytes)
        .map_err(|error| io_error(path, error, "read"))?;
    Ok(bytes)
}

fn stat_file(path: &Path) -> Result<HostResourceFileStat, HostResourceStoreError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error, "stat"))?;

    if !metadata.is_file() {
        return Err(HostResourceStoreError::not_found(format!(
            "Host resource not found: {}",
            path.display()
        )));
    }

    Ok(HostResourceFileStat {
        len: metadata.len(),
        mime_type: mime_type_for_path(path),
    })
}

fn mime_type_for_path(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn io_error(path: &Path, error: std::io::Error, operation: &str) -> HostResourceStoreError {
    match error.kind() {
        std::io::ErrorKind::NotFound => HostResourceStoreError::not_found(format!(
            "Host resource not found: {}",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => HostResourceStoreError::forbidden(format!(
            "Host resource is not readable: {}",
            path.display()
        )),
        _ => HostResourceStoreError::internal(format!(
            "Failed to {operation} host resource '{}': {}",
            path.display(),
            error
        )),
    }
}

fn host_resource_error_from_domain(error: DomainError) -> HostResourceStoreError {
    match error {
        DomainError::NotFound(message) => HostResourceStoreError::NotFound(message),
        error => HostResourceStoreError::Internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(test_name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("tauritavern-{test_name}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn roots(root: &Path) -> HostResourceRoots {
        HostResourceRoots {
            user_css_file: root.join("_css").join("user.css"),
            local_extensions_dir: root.join("default-user").join("extensions"),
            global_extensions_dir: root.join("extensions").join("third-party"),
            characters_dir: root.join("characters"),
            avatars_dir: root.join("User Avatars"),
            backgrounds_dir: root.join("backgrounds"),
            assets_dir: root.join("assets"),
            user_images_dir: root.join("user").join("images"),
            user_files_dir: root.join("user").join("files"),
            thumbnails_bg_dir: root.join("thumbnails").join("bg"),
            thumbnails_avatar_dir: root.join("thumbnails").join("avatar"),
            thumbnails_persona_dir: root.join("thumbnails").join("persona"),
        }
    }

    #[test]
    fn third_party_assets_prefer_local_over_global() {
        let temp = TempDirGuard::new("host-resources-third-party-local");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let local_file = temp
            .path
            .join("default-user/extensions/mobile/manifest.json");
        let global_file = temp
            .path
            .join("extensions/third-party/mobile/manifest.json");
        fs::create_dir_all(local_file.parent().expect("local parent")).expect("local dir");
        fs::create_dir_all(global_file.parent().expect("global parent")).expect("global dir");
        fs::write(&local_file, br#"{"source":"local"}"#).expect("local file");
        fs::write(&global_file, br#"{"source":"global"}"#).expect("global file");

        let asset = store
            .read_third_party_asset("mobile", Path::new("manifest.json"), None)
            .expect("read asset");

        assert_eq!(asset.bytes, br#"{"source":"local"}"#);
        assert_eq!(asset.mime_type, "application/json");
    }

    #[test]
    fn third_party_asset_max_len_applies_to_selected_local_asset() {
        let temp = TempDirGuard::new("host-resources-third-party-max-len");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let local_file = temp.path.join("default-user/extensions/mobile/app.js");
        let global_file = temp.path.join("extensions/third-party/mobile/app.js");
        fs::create_dir_all(local_file.parent().expect("local parent")).expect("local dir");
        fs::create_dir_all(global_file.parent().expect("global parent")).expect("global dir");
        fs::write(&local_file, b"large").expect("local file");
        fs::write(&global_file, b"ok").expect("global file");

        let result = store.read_third_party_asset("mobile", Path::new("app.js"), Some(2));

        assert!(matches!(
            result,
            Err(HostResourceStoreError::PayloadTooLarge {
                size_bytes: 5,
                limit_bytes: 2,
            })
        ));
    }

    #[test]
    fn user_data_range_reads_only_requested_bytes() {
        let temp = TempDirGuard::new("host-resources-user-data-range");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("backgrounds").join("a.bin");
        fs::create_dir_all(file.parent().expect("background parent")).expect("background dir");
        fs::write(&file, b"abcd").expect("background file");

        let bytes = store
            .read_user_data_asset_range(
                UserDataAssetKind::Background,
                Path::new("a.bin"),
                ByteRange { start: 1, end: 2 },
            )
            .expect("read range");

        assert_eq!(bytes, b"bc");
    }

    #[cfg(unix)]
    #[test]
    fn user_data_symlink_to_external_file_is_allowed() {
        let temp = TempDirGuard::new("host-resources-user-data-symlink-allowed");
        let external = TempDirGuard::new("host-resources-user-data-symlink-external");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let outside = external.path.join("outside.txt");
        let link = temp.path.join("backgrounds").join("escape.txt");
        fs::create_dir_all(link.parent().expect("background parent")).expect("background dir");
        fs::write(&outside, b"secret").expect("outside file");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        let bytes = store
            .read_user_data_asset(UserDataAssetKind::Background, Path::new("escape.txt"))
            .expect("read symlinked file");

        assert_eq!(bytes, b"secret");
    }

    #[test]
    fn animated_thumbnail_requests_return_original_asset() {
        let temp = TempDirGuard::new("host-resources-thumbnail-original");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("characters").join("a.gif");
        fs::create_dir_all(file.parent().expect("characters parent")).expect("characters dir");
        fs::write(&file, b"gif").expect("gif file");

        let asset = store
            .read_thumbnail_asset(ThumbnailAssetRequest {
                kind: ThumbnailKind::Avatar,
                file: "a.gif".to_string(),
                use_thumbnails: true,
            })
            .expect("thumbnail asset");

        assert_eq!(asset.bytes, b"gif");
        assert_eq!(asset.mime_type, "image/gif");
    }
}
