use tt_domain::models::character::Character;

pub const CHARACTER_CREATE_WARNING_AVATAR_IMPORT_FAILED: &str = "avatar-import-failed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterCreateWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CharacterCreateResult {
    pub character: Character,
    pub warnings: Vec<CharacterCreateWarning>,
}

/// Character chat information
#[derive(Debug, Clone)]
pub struct CharacterChat {
    pub file_name: String,
    pub file_size: String,
    pub chat_items: usize,
    pub last_message: String,
    pub last_message_date: i64,
}
