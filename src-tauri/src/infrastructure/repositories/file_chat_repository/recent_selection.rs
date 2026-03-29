use tokio::fs;

use crate::domain::errors::DomainError;

use super::FileChatRepository;
use super::summary::ChatFileDescriptor;

#[derive(Clone)]
struct RankedChatDescriptor {
    modified_millis: i64,
    descriptor: ChatFileDescriptor,
}

impl FileChatRepository {
    pub(super) fn character_recent_pin_key(
        character_name: &str,
        file_name: &str,
    ) -> Option<String> {
        let normalized_character = character_name.trim();
        let normalized_file = file_name.trim();
        if normalized_character.is_empty() || normalized_file.is_empty() {
            return None;
        }

        Some(format!(
            "{}/{}",
            normalized_character,
            Self::normalize_jsonl_file_name(normalized_file)
        ))
    }

    pub(super) fn group_recent_pin_key(chat_id: &str) -> Option<String> {
        let normalized_chat_id = chat_id.trim();
        if normalized_chat_id.is_empty() {
            return None;
        }

        let normalized_file = Self::normalize_jsonl_file_name(normalized_chat_id);
        Some(Self::strip_jsonl_extension(&normalized_file).to_string())
    }

    pub(super) async fn select_recent_descriptors<F>(
        &self,
        descriptors: Vec<ChatFileDescriptor>,
        max_entries: usize,
        is_pinned: F,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError>
    where
        F: Fn(&ChatFileDescriptor) -> bool,
    {
        let mut pinned = Vec::new();
        let mut non_pinned = Vec::new();

        for descriptor in descriptors {
            let metadata = fs::metadata(&descriptor.path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat metadata {:?}: {}",
                    descriptor.path, error
                ))
            })?;
            let modified_millis = Self::file_signature_from_metadata(&metadata).modified_millis;

            let ranked = RankedChatDescriptor {
                modified_millis,
                descriptor,
            };

            if is_pinned(&ranked.descriptor) {
                pinned.push(ranked);
            } else {
                non_pinned.push(ranked);
            }
        }

        pinned.sort_by(|a, b| b.modified_millis.cmp(&a.modified_millis));
        non_pinned.sort_by(|a, b| b.modified_millis.cmp(&a.modified_millis));

        let non_pinned_limit = max_entries.saturating_sub(pinned.len());
        let mut selected: Vec<ChatFileDescriptor> =
            pinned.into_iter().map(|entry| entry.descriptor).collect();
        selected.extend(
            non_pinned
                .into_iter()
                .take(non_pinned_limit)
                .map(|entry| entry.descriptor),
        );

        Ok(selected)
    }
}
