use chrono::Utc;

use crate::domain::errors::DomainError;
use crate::domain::models::character::sanitize_filename;
use crate::domain::models::chat::{humanized_date, truncate_chat_file_stem_prefix};

use super::FileChatRepository;

impl FileChatRepository {
    pub(super) fn next_import_chat_file_stem(
        &self,
        character_name: &str,
        character_display_name: &str,
        index: usize,
    ) -> Result<String, DomainError> {
        let display_name = sanitize_filename(character_display_name);
        let fallback_name = sanitize_filename(character_name);
        let base_name = if display_name.is_empty() {
            fallback_name
        } else {
            display_name
        };

        let import_suffix = format!(" - {} imported", humanized_date(Utc::now()));
        let mut ordinal = if index == 0 { 1 } else { index + 1 };
        loop {
            let suffix = if ordinal == 1 {
                import_suffix.clone()
            } else {
                format!("{} {}", import_suffix, ordinal)
            };
            let candidate = format!(
                "{}{}",
                truncate_chat_file_stem_prefix(&base_name, &suffix),
                suffix
            );
            if !self.get_chat_path(character_name, &candidate)?.exists() {
                return Ok(candidate);
            }
            ordinal += 1;
        }
    }

    pub(super) fn next_group_chat_id(&self) -> Result<String, DomainError> {
        let base = humanized_date(Utc::now());
        let mut candidate = base.clone();
        let mut suffix = 1;
        while self.get_group_chat_path(&candidate)?.exists() {
            candidate = format!("{} {}", base, suffix + 1);
            suffix += 1;
        }
        Ok(candidate)
    }
}
