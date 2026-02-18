use chrono::Utc;

use crate::domain::models::character::sanitize_filename;
use crate::domain::models::chat::humanized_date;

use super::FileChatRepository;

impl FileChatRepository {
    pub(super) fn next_import_chat_file_stem(
        &self,
        character_name: &str,
        character_display_name: &str,
        index: usize,
    ) -> String {
        let display_name = sanitize_filename(character_display_name);
        let fallback_name = sanitize_filename(character_name);
        let base_name = if display_name.is_empty() {
            fallback_name
        } else {
            display_name
        };

        let mut base = format!("{} - {} imported", base_name, humanized_date(Utc::now()));
        if index > 0 {
            base = format!("{} {}", base, index + 1);
        }

        let mut candidate = base.clone();
        let mut suffix = 1;
        while self.get_chat_path(character_name, &candidate).exists() {
            candidate = format!("{} {}", base, suffix + 1);
            suffix += 1;
        }

        candidate
    }

    pub(super) fn next_group_chat_id(&self) -> String {
        let base = humanized_date(Utc::now());
        let mut candidate = base.clone();
        let mut suffix = 1;
        while self.get_group_chat_path(&candidate).exists() {
            candidate = format!("{} {}", base, suffix + 1);
            suffix += 1;
        }
        candidate
    }
}
