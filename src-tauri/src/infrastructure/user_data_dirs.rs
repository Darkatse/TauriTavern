use std::path::{Path, PathBuf};

use crate::domain::models::user_directory::UserDirectory;

#[derive(Debug, Clone)]
pub struct DefaultUserWebDirs {
    pub characters_dir: PathBuf,
    pub avatars_dir: PathBuf,
    pub backgrounds_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub user_images_dir: PathBuf,
    pub user_files_dir: PathBuf,
    pub thumbnails_bg_dir: PathBuf,
    pub thumbnails_avatar_dir: PathBuf,
    pub thumbnails_persona_dir: PathBuf,
}

impl DefaultUserWebDirs {
    pub fn from_data_root(data_root: impl AsRef<Path>) -> Self {
        let directories = UserDirectory::default_user(data_root.as_ref());
        Self {
            characters_dir: directories.characters,
            avatars_dir: directories.avatars,
            backgrounds_dir: directories.backgrounds,
            assets_dir: directories.assets,
            user_images_dir: directories.user_images,
            user_files_dir: directories.files,
            thumbnails_bg_dir: directories.thumbnails_bg,
            thumbnails_avatar_dir: directories.thumbnails_avatar,
            thumbnails_persona_dir: directories.thumbnails_persona,
        }
    }
}
