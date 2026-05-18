use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DataRootContentDirs {
    pub user_css_file: PathBuf,
}

impl DataRootContentDirs {
    pub fn from_data_root(data_root: impl AsRef<Path>) -> Self {
        Self {
            user_css_file: data_root.as_ref().join("_css").join("user.css"),
        }
    }
}
