use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct HostResourceRoots {
    pub(crate) user_css_file: PathBuf,
    pub(crate) local_extensions_dir: PathBuf,
    pub(crate) global_extensions_dir: PathBuf,
    pub(crate) characters_dir: PathBuf,
    pub(crate) avatars_dir: PathBuf,
    pub(crate) backgrounds_dir: PathBuf,
    pub(crate) assets_dir: PathBuf,
    pub(crate) user_images_dir: PathBuf,
    pub(crate) user_files_dir: PathBuf,
    pub(crate) thumbnails_bg_dir: PathBuf,
    pub(crate) thumbnails_avatar_dir: PathBuf,
    pub(crate) thumbnails_persona_dir: PathBuf,
}
