use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::persistence::thumbnail_cache::{
    OpenThumbnailSource, ThumbnailConfig, ThumbnailSourceSnapshot, open_thumbnail_or_original_sync,
    thumbnail_cache_identity,
};
use crate::thumbnails::{avatar_thumbnail_config, background_thumbnail_config};
use tt_contracts::client_asset_paths::UserDataAssetKind;
use tt_contracts::range::ByteRange;
use tt_domain::errors::DomainError;
use tt_domain::models::user_directory::UserDirectory;
use tt_ports::host_resource::{
    HostResourceAssetStore, HostResourceBody, HostResourceSourceMetadata,
    HostResourceSourceRequest, HostResourceSourceRevision, HostResourceStoreError,
    OpenedHostResource, ThumbnailAssetRequest, ThumbnailKind, ThumbnailSelection,
};

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
pub struct FilesystemHostResourceStore {
    roots: HostResourceRoots,
}

struct OpenHostResourceFile {
    path: PathBuf,
    file: fs::File,
    content_type: String,
    content_length: u64,
    last_modified: SystemTime,
}

struct FileHostResourceBody {
    path: PathBuf,
    file: fs::File,
    content_length: u64,
    last_modified: SystemTime,
}

impl FileHostResourceBody {
    fn ensure_source_unchanged(&self) -> Result<(), HostResourceStoreError> {
        let metadata = self
            .file
            .metadata()
            .map_err(|error| io_error(&self.path, error, "stat after read"))?;
        let last_modified = metadata
            .modified()
            .map_err(|error| io_error(&self.path, error, "read modification time after read"))?;
        if metadata.len() != self.content_length || last_modified != self.last_modified {
            return Err(HostResourceStoreError::internal(format!(
                "Host resource changed while being read: {}",
                self.path.display()
            )));
        }
        Ok(())
    }
}

impl HostResourceBody for FileHostResourceBody {
    fn read(
        mut self: Box<Self>,
        range: Option<ByteRange>,
    ) -> Result<Vec<u8>, HostResourceStoreError> {
        let Some(range) = range else {
            let mut bytes = Vec::new();
            (&mut self.file)
                .take(self.content_length.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|error| io_error(&self.path, error, "read"))?;
            self.ensure_source_unchanged()?;
            return Ok(bytes);
        };

        let range_len = usize::try_from(range.byte_len())
            .map_err(|_| HostResourceStoreError::internal("Range is too large to serve"))?;
        self.file
            .seek(std::io::SeekFrom::Start(range.start()))
            .map_err(|error| io_error(&self.path, error, "seek"))?;

        let mut bytes = vec![0u8; range_len];
        self.file
            .read_exact(&mut bytes)
            .map_err(|error| io_error(&self.path, error, "read range"))?;
        self.ensure_source_unchanged()?;
        Ok(bytes)
    }
}

impl OpenHostResourceFile {
    fn into_resource(self, revision_scope: &[u8]) -> OpenedHostResource {
        let metadata = HostResourceSourceMetadata {
            content_type: self.content_type.clone(),
            content_length: self.content_length,
            last_modified: self.last_modified,
            revision: source_revision(
                revision_scope,
                self.content_length,
                self.last_modified,
                &self.content_type,
            ),
        };
        let body = FileHostResourceBody {
            path: self.path,
            file: self.file,
            content_length: self.content_length,
            last_modified: self.last_modified,
        };
        OpenedHostResource::new(metadata, Box::new(body))
    }
}

impl FilesystemHostResourceStore {
    pub fn from_data_root(data_root: impl AsRef<Path>) -> Self {
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

    fn open_third_party_asset(
        &self,
        extension_folder: &str,
        relative_path: &Path,
    ) -> Result<OpenedHostResource, HostResourceStoreError> {
        for (root, revision_scope) in [
            (
                &self.roots.local_extensions_dir,
                b"third-party-local".as_slice(),
            ),
            (
                &self.roots.global_extensions_dir,
                b"third-party-global".as_slice(),
            ),
        ] {
            let path = root.join(extension_folder).join(relative_path);
            match open_file(&path) {
                Ok(opened) => return Ok(opened.into_resource(revision_scope)),
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

    fn open_thumbnail_asset(
        &self,
        request: &ThumbnailAssetRequest,
    ) -> Result<OpenedHostResource, HostResourceStoreError> {
        let (original_root, thumbnail_root, config) = self.thumbnail_paths(request);
        let original_path = original_root.join(&request.file);
        let thumbnail_path = thumbnail_root.join(&request.file);
        let original = open_file(&original_path)?;

        if request.selection == ThumbnailSelection::Original {
            return Ok(original.into_resource(thumbnail_original_revision_scope(request.kind)));
        }

        let original_revision = source_revision(
            thumbnail_original_revision_scope(request.kind),
            original.content_length,
            original.last_modified,
            &original.content_type,
        );
        let cache_identity = thumbnail_cache_identity(original_revision.as_bytes(), config);
        let source_snapshot = ThumbnailSourceSnapshot {
            content_length: original.content_length,
            last_modified: original.last_modified,
        };
        let selected = open_thumbnail_or_original_sync(
            original.file,
            &original_path,
            source_snapshot,
            &thumbnail_path,
            config,
            &cache_identity,
            request.selection == ThumbnailSelection::RequireGenerated,
        )
        .map_err(host_resource_error_from_domain)?;

        match selected {
            OpenThumbnailSource::Original(file) => {
                Ok(open_file_handle(&original_path, file, None)?
                    .into_resource(thumbnail_original_revision_scope(request.kind)))
            }
            OpenThumbnailSource::CachedJpeg(file) => {
                Ok(open_file_handle(&thumbnail_path, file, Some("image/jpeg"))?
                    .into_resource(&cache_identity))
            }
        }
    }
}

impl HostResourceAssetStore for FilesystemHostResourceStore {
    fn open(
        &self,
        request: HostResourceSourceRequest<'_>,
    ) -> Result<OpenedHostResource, HostResourceStoreError> {
        match request {
            HostResourceSourceRequest::UserCss => {
                Ok(open_file(&self.roots.user_css_file)?.into_resource(b"user-css"))
            }
            HostResourceSourceRequest::ThirdParty {
                extension_folder,
                relative_path,
            } => self.open_third_party_asset(extension_folder, relative_path),
            HostResourceSourceRequest::UserData {
                kind,
                relative_path,
            } => Ok(open_file(&self.user_data_root(kind).join(relative_path))?
                .into_resource(user_data_revision_scope(kind))),
            HostResourceSourceRequest::Thumbnail(request) => self.open_thumbnail_asset(request),
        }
    }
}

fn open_file(path: &Path) -> Result<OpenHostResourceFile, HostResourceStoreError> {
    let file = fs::File::open(path).map_err(|error| io_error(path, error, "open"))?;
    open_file_handle(path, file, None)
}

fn open_file_handle(
    path: &Path,
    file: fs::File,
    content_type: Option<&str>,
) -> Result<OpenHostResourceFile, HostResourceStoreError> {
    let metadata = file
        .metadata()
        .map_err(|error| io_error(path, error, "stat"))?;

    if !metadata.is_file() {
        return Err(HostResourceStoreError::not_found(format!(
            "Host resource not found: {}",
            path.display()
        )));
    }

    let last_modified = metadata
        .modified()
        .map_err(|error| io_error(path, error, "read modification time"))?;
    Ok(OpenHostResourceFile {
        path: path.to_path_buf(),
        file,
        content_type: content_type
            .map(str::to_owned)
            .unwrap_or_else(|| mime_type_for_path(path)),
        content_length: metadata.len(),
        last_modified,
    })
}

fn source_revision(
    scope: &[u8],
    content_length: u64,
    last_modified: SystemTime,
    content_type: &str,
) -> HostResourceSourceRevision {
    let mut revision = Vec::with_capacity(scope.len() + content_type.len() + 32);
    revision.extend_from_slice(b"host-source-v1\0");
    revision.extend_from_slice(scope);
    revision.push(0);
    revision.extend_from_slice(&content_length.to_be_bytes());
    match last_modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            revision.push(1);
            revision.extend_from_slice(&duration.as_secs().to_be_bytes());
            revision.extend_from_slice(&duration.subsec_nanos().to_be_bytes());
        }
        Err(error) => {
            let duration = error.duration();
            revision.push(0);
            revision.extend_from_slice(&duration.as_secs().to_be_bytes());
            revision.extend_from_slice(&duration.subsec_nanos().to_be_bytes());
        }
    }
    revision.extend_from_slice(content_type.as_bytes());
    HostResourceSourceRevision::new(revision)
}

fn mime_type_for_path(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn user_data_revision_scope(kind: UserDataAssetKind) -> &'static [u8] {
    match kind {
        UserDataAssetKind::Character => b"user-data-character",
        UserDataAssetKind::Persona => b"user-data-persona",
        UserDataAssetKind::Background => b"user-data-background",
        UserDataAssetKind::Asset => b"user-data-asset",
        UserDataAssetKind::UserImage => b"user-data-image",
        UserDataAssetKind::UserFile => b"user-data-file",
    }
}

fn thumbnail_original_revision_scope(kind: ThumbnailKind) -> &'static [u8] {
    match kind {
        ThumbnailKind::Avatar => b"thumbnail-avatar-original",
        ThumbnailKind::Persona => b"thumbnail-persona-original",
        ThumbnailKind::Background => b"thumbnail-background-original",
    }
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
    use std::fs::FileTimes;

    use image::codecs::gif::GifEncoder;
    use image::{Frame, ImageBuffer, Rgb, Rgba};
    use tt_contracts::range::parse_single_range_header;

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

        let opened = store
            .open(HostResourceSourceRequest::ThirdParty {
                extension_folder: "mobile",
                relative_path: Path::new("manifest.json"),
            })
            .expect("open asset");
        assert_eq!(opened.metadata.content_type, "application/json");
        let bytes = opened.read(None).expect("read asset");

        assert_eq!(bytes, br#"{"source":"local"}"#);
    }

    #[test]
    fn third_party_selection_and_revision_change_when_local_asset_is_removed() {
        let temp = TempDirGuard::new("host-resources-third-party-max-len");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let local_file = temp.path.join("default-user/extensions/mobile/app.js");
        let global_file = temp.path.join("extensions/third-party/mobile/app.js");
        fs::create_dir_all(local_file.parent().expect("local parent")).expect("local dir");
        fs::create_dir_all(global_file.parent().expect("global parent")).expect("global dir");
        fs::write(&local_file, b"large").expect("local file");
        fs::write(&global_file, b"ok").expect("global file");

        let local = store
            .open(HostResourceSourceRequest::ThirdParty {
                extension_folder: "mobile",
                relative_path: Path::new("app.js"),
            })
            .expect("local");
        let local_revision = local.metadata.revision.clone();
        assert_eq!(local.metadata.content_length, 5);
        fs::remove_file(&local_file).expect("remove local");
        let global = store
            .open(HostResourceSourceRequest::ThirdParty {
                extension_folder: "mobile",
                relative_path: Path::new("app.js"),
            })
            .expect("global");

        assert_eq!(global.metadata.content_length, 2);
        assert_ne!(local_revision, global.metadata.revision);
    }

    #[test]
    fn user_data_range_reads_only_requested_bytes() {
        let temp = TempDirGuard::new("host-resources-user-data-range");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("backgrounds").join("a.bin");
        fs::create_dir_all(file.parent().expect("background parent")).expect("background dir");
        fs::write(&file, b"abcd").expect("background file");

        let range = parse_single_range_header("bytes=1-2", 4).expect("parse range");
        let bytes = store
            .open(HostResourceSourceRequest::UserData {
                kind: UserDataAssetKind::Background,
                relative_path: Path::new("a.bin"),
            })
            .expect("open range")
            .read(Some(range))
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
            .open(HostResourceSourceRequest::UserData {
                kind: UserDataAssetKind::Background,
                relative_path: Path::new("escape.txt"),
            })
            .expect("open symlinked file")
            .read(None)
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

        let request = ThumbnailAssetRequest {
            kind: ThumbnailKind::Avatar,
            file: "a.gif".to_string(),
            selection: ThumbnailSelection::PreferGenerated,
        };
        let opened = store
            .open(HostResourceSourceRequest::Thumbnail(&request))
            .expect("thumbnail asset");
        assert_eq!(opened.metadata.content_type, "image/gif");
        let bytes = opened.read(None).expect("read thumbnail");

        assert_eq!(bytes, b"gif");
    }

    #[test]
    fn static_thumbnail_returns_opened_cached_jpeg() {
        let temp = TempDirGuard::new("host-resources-thumbnail-generated");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("characters").join("a.png");
        fs::create_dir_all(file.parent().expect("characters parent")).expect("characters dir");
        ImageBuffer::from_pixel(2, 2, Rgb([255u8, 0, 0]))
            .save(&file)
            .expect("source image");
        let request = ThumbnailAssetRequest {
            kind: ThumbnailKind::Avatar,
            file: "a.png".to_string(),
            selection: ThumbnailSelection::PreferGenerated,
        };

        let opened = store
            .open(HostResourceSourceRequest::Thumbnail(&request))
            .expect("open thumbnail");
        assert_eq!(opened.metadata.content_type, "image/jpeg");
        let bytes = opened.read(None).expect("read thumbnail");

        assert!(bytes.starts_with(&[0xff, 0xd8]));
    }

    #[test]
    fn failed_thumbnail_generation_falls_back_before_representation_selection() {
        let temp = TempDirGuard::new("host-resources-thumbnail-fallback");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("characters").join("a.png");
        fs::create_dir_all(file.parent().expect("characters parent")).expect("characters dir");
        fs::write(&file, b"not-an-image").expect("invalid source image");
        let request = ThumbnailAssetRequest {
            kind: ThumbnailKind::Avatar,
            file: "a.png".to_string(),
            selection: ThumbnailSelection::PreferGenerated,
        };

        let opened = store
            .open(HostResourceSourceRequest::Thumbnail(&request))
            .expect("fallback original");
        assert_eq!(opened.metadata.content_type, "image/png");
        assert_eq!(opened.read(None).expect("read original"), b"not-an-image");
    }

    #[test]
    fn required_static_preview_decodes_an_animated_image_to_jpeg() {
        let temp = TempDirGuard::new("host-resources-thumbnail-required-static");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("backgrounds").join("a.gif");
        fs::create_dir_all(file.parent().expect("background parent")).expect("background dir");
        let mut encoder = GifEncoder::new(fs::File::create(&file).expect("create gif"));
        encoder
            .encode_frame(Frame::new(ImageBuffer::from_pixel(
                2,
                2,
                Rgba([255u8, 0, 0, 255]),
            )))
            .expect("encode gif");
        drop(encoder);
        let request = ThumbnailAssetRequest {
            kind: ThumbnailKind::Background,
            file: "a.gif".to_string(),
            selection: ThumbnailSelection::RequireGenerated,
        };

        let opened = store
            .open(HostResourceSourceRequest::Thumbnail(&request))
            .expect("static preview");
        assert_eq!(opened.metadata.content_type, "image/jpeg");
        assert!(
            opened
                .read(None)
                .expect("read preview")
                .starts_with(&[0xff, 0xd8])
        );
    }

    #[test]
    fn required_static_preview_fails_for_unsupported_video() {
        let temp = TempDirGuard::new("host-resources-thumbnail-required-video");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("backgrounds").join("a.mp4");
        fs::create_dir_all(file.parent().expect("background parent")).expect("background dir");
        fs::write(&file, b"video").expect("video file");
        let request = ThumbnailAssetRequest {
            kind: ThumbnailKind::Background,
            file: "a.mp4".to_string(),
            selection: ThumbnailSelection::RequireGenerated,
        };

        let error = match store.open(HostResourceSourceRequest::Thumbnail(&request)) {
            Ok(_) => panic!("video cannot produce an image preview"),
            Err(error) => error,
        };

        assert!(matches!(error, HostResourceStoreError::Internal(_)));
    }

    #[test]
    fn generated_thumbnail_revision_tracks_source_length_when_mtime_is_preserved() {
        let temp = TempDirGuard::new("host-resources-thumbnail-source-revision");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("characters").join("a.png");
        fs::create_dir_all(file.parent().expect("characters parent")).expect("characters dir");
        ImageBuffer::from_pixel(2, 2, Rgb([255u8, 0, 0]))
            .save(&file)
            .expect("first source image");
        let request = ThumbnailAssetRequest {
            kind: ThumbnailKind::Avatar,
            file: "a.png".to_string(),
            selection: ThumbnailSelection::PreferGenerated,
        };

        let first = store
            .open(HostResourceSourceRequest::Thumbnail(&request))
            .expect("first thumbnail");
        let first_revision = first.metadata.revision.clone();
        let first_bytes = first.read(None).expect("first bytes");
        let source_metadata = fs::metadata(&file).expect("source metadata");
        let source_mtime = source_metadata.modified().expect("source mtime");

        let reopened = store
            .open(HostResourceSourceRequest::Thumbnail(&request))
            .expect("reopen thumbnail");
        assert_eq!(reopened.metadata.revision, first_revision);

        ImageBuffer::from_pixel(3, 2, Rgb([0u8, 0, 255]))
            .save(&file)
            .expect("replacement source image");
        fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .expect("open replacement source")
            .set_times(FileTimes::new().set_modified(source_mtime))
            .expect("preserve source mtime");
        assert_ne!(
            fs::metadata(&file).expect("replacement metadata").len(),
            source_metadata.len()
        );

        let changed = store
            .open(HostResourceSourceRequest::Thumbnail(&request))
            .expect("changed thumbnail");
        assert_ne!(changed.metadata.revision, first_revision);
        assert_ne!(changed.read(None).expect("changed bytes"), first_bytes);
    }

    #[test]
    fn source_revision_is_stable_across_reopen() {
        let temp = TempDirGuard::new("host-resources-stable-revision");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("backgrounds").join("a.bin");
        fs::create_dir_all(file.parent().expect("background parent")).expect("background dir");
        fs::write(&file, b"abcd").expect("background file");
        let request = || HostResourceSourceRequest::UserData {
            kind: UserDataAssetKind::Background,
            relative_path: Path::new("a.bin"),
        };

        let first = store.open(request()).expect("first");
        let second = store.open(request()).expect("second");

        assert_eq!(first.metadata.revision, second.metadata.revision);
    }

    #[test]
    fn opened_handle_keeps_metadata_and_body_on_atomic_replacement() {
        let temp = TempDirGuard::new("host-resources-open-handle-replacement");
        let store = FilesystemHostResourceStore::new(roots(&temp.path));
        let file = temp.path.join("backgrounds").join("a.bin");
        let replacement = temp.path.join("backgrounds").join("replacement.bin");
        fs::create_dir_all(file.parent().expect("background parent")).expect("background dir");
        fs::write(&file, b"old").expect("old file");
        fs::write(&replacement, b"newer").expect("new file");

        let opened = store
            .open(HostResourceSourceRequest::UserData {
                kind: UserDataAssetKind::Background,
                relative_path: Path::new("a.bin"),
            })
            .expect("open old");
        tt_adapter_storage_core::file_system::replace_file_with_fallback_sync(&replacement, &file)
            .expect("replace path");

        assert_eq!(opened.metadata.content_length, 3);
        assert_eq!(opened.read(None).expect("read old handle"), b"old");
    }

    #[test]
    fn full_read_rejects_in_place_growth() {
        use std::io::Write as _;

        let temp = TempDirGuard::new("host-resources-in-place-growth");
        let file_path = temp.path.join("growing.bin");
        fs::write(&file_path, b"old").expect("initial file");
        let body = FileHostResourceBody {
            path: file_path.clone(),
            file: fs::File::open(&file_path).expect("open initial file"),
            content_length: 3,
            last_modified: fs::metadata(&file_path)
                .expect("initial metadata")
                .modified()
                .expect("initial mtime"),
        };
        fs::OpenOptions::new()
            .append(true)
            .open(&file_path)
            .expect("open append handle")
            .write_all(b"new content that must not be read")
            .expect("grow file");

        let error = Box::new(body)
            .read(None)
            .expect_err("in-place growth must fail");

        assert!(matches!(error, HostResourceStoreError::Internal(_)));
    }
}
