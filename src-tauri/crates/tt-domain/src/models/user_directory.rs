use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Represents the directory structure for a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDirectory {
    pub handle: String,
    pub root: PathBuf,
    pub thumbnails: PathBuf,
    pub thumbnails_bg: PathBuf,
    pub thumbnails_avatar: PathBuf,
    pub thumbnails_persona: PathBuf,
    pub worlds: PathBuf,
    pub user: PathBuf,
    pub avatars: PathBuf,
    pub user_images: PathBuf,
    pub groups: PathBuf,
    pub group_chats: PathBuf,
    pub chats: PathBuf,
    pub characters: PathBuf,
    pub backgrounds: PathBuf,
    pub novel_ai_settings: PathBuf,
    pub kobold_ai_settings: PathBuf,
    pub openai_settings: PathBuf,
    pub textgen_settings: PathBuf,
    pub themes: PathBuf,
    pub moving_ui: PathBuf,
    pub extensions: PathBuf,
    pub instruct: PathBuf,
    pub context: PathBuf,
    pub quick_replies: PathBuf,
    pub assets: PathBuf,
    pub comfy_workflows: PathBuf,
    pub files: PathBuf,
    pub vectors: PathBuf,
    pub backups: PathBuf,
    pub sysprompt: PathBuf,
    pub reasoning: PathBuf,
}

impl UserDirectory {
    /// Create a new UserDirectory instance for a specific user handle
    pub fn new(data_root: &Path, handle: &str) -> Self {
        let root = data_root.join(handle);

        Self {
            handle: handle.to_string(),
            root: root.clone(),
            thumbnails: root.join("thumbnails"),
            thumbnails_bg: root.join("thumbnails/bg"),
            thumbnails_avatar: root.join("thumbnails/avatar"),
            thumbnails_persona: root.join("thumbnails/persona"),
            worlds: root.join("worlds"),
            user: root.join("user"),
            avatars: root.join("User Avatars"),
            user_images: root.join("user/images"),
            groups: root.join("groups"),
            group_chats: root.join("group chats"),
            chats: root.join("chats"),
            characters: root.join("characters"),
            backgrounds: root.join("backgrounds"),
            novel_ai_settings: root.join("NovelAI Settings"),
            kobold_ai_settings: root.join("KoboldAI Settings"),
            openai_settings: root.join("OpenAI Settings"),
            textgen_settings: root.join("TextGen Settings"),
            themes: root.join("themes"),
            moving_ui: root.join("movingUI"),
            extensions: root.join("extensions"),
            instruct: root.join("instruct"),
            context: root.join("context"),
            quick_replies: root.join("QuickReplies"),
            assets: root.join("assets"),
            comfy_workflows: root.join("user/workflows"),
            files: root.join("user/files"),
            vectors: root.join("vectors"),
            backups: root.join("backups"),
            sysprompt: root.join("sysprompt"),
            reasoning: root.join("reasoning"),
        }
    }

    /// Create a new UserDirectory instance for the default user
    pub fn default_user(data_root: &Path) -> Self {
        Self::new(data_root, "default-user")
    }

    /// Get all directory paths as a vector
    pub fn all_directories(&self) -> Vec<&Path> {
        vec![
            &self.root,
            &self.thumbnails,
            &self.thumbnails_bg,
            &self.thumbnails_avatar,
            &self.thumbnails_persona,
            &self.worlds,
            &self.user,
            &self.avatars,
            &self.user_images,
            &self.groups,
            &self.group_chats,
            &self.chats,
            &self.characters,
            &self.backgrounds,
            &self.novel_ai_settings,
            &self.kobold_ai_settings,
            &self.openai_settings,
            &self.textgen_settings,
            &self.themes,
            &self.moving_ui,
            &self.extensions,
            &self.instruct,
            &self.context,
            &self.quick_replies,
            &self.assets,
            &self.comfy_workflows,
            &self.files,
            &self.vectors,
            &self.backups,
            &self.sysprompt,
            &self.reasoning,
        ]
    }
}
