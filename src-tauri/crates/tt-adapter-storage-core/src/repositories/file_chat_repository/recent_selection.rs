use std::cmp::Ordering;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use tt_domain::errors::DomainError;
use tt_domain::models::chat::strip_jsonl_extension;

use super::FileChatRepository;
use super::summary::ChatFileDescriptor;

#[derive(Clone)]
struct RankedChatDescriptor {
    date_millis: i64,
    descriptor: ChatFileDescriptor,
}

fn compare_ranked_chat_descriptors(a: &RankedChatDescriptor, b: &RankedChatDescriptor) -> Ordering {
    b.date_millis
        .cmp(&a.date_millis)
        .then_with(|| {
            a.descriptor
                .character_name
                .cmp(&b.descriptor.character_name)
        })
        .then_with(|| a.descriptor.file_name.cmp(&b.descriptor.file_name))
}

impl FileChatRepository {
    pub(super) fn character_recent_pin_key(
        character_name: &str,
        file_name: &str,
    ) -> Option<String> {
        let normalized_character = character_name.trim();
        if normalized_character.is_empty() || file_name.trim().is_empty() {
            return None;
        }

        Some(format!(
            "{}/{}",
            normalized_character,
            Self::normalize_jsonl_file_name(file_name).ok()?
        ))
    }

    pub(super) fn group_recent_pin_key(chat_id: &str) -> Option<String> {
        if chat_id.trim().is_empty() {
            return None;
        }

        let normalized_file = Self::normalize_jsonl_file_name(chat_id).ok()?;
        Some(strip_jsonl_extension(&normalized_file).to_string())
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
            if is_pinned(&descriptor) {
                pinned.push(descriptor);
            } else {
                non_pinned.push(descriptor);
            }
        }

        let non_pinned_limit = max_entries.saturating_sub(pinned.len());
        if non_pinned_limit == 0 {
            return Ok(pinned);
        }

        let mut ranked_non_pinned = Vec::new();
        let semaphore = Arc::new(Semaphore::new(Self::chat_stats_parallelism()));
        let mut jobs = JoinSet::new();

        for descriptor in non_pinned {
            let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
                DomainError::InternalError("Recent chat stats scanner gate closed".to_string())
            })?;
            let summary_cache = self.summary_cache.clone();

            jobs.spawn(async move {
                let _permit = permit;
                let date_millis = Self::get_chat_stats_date(&summary_cache, &descriptor).await?;
                Ok::<_, DomainError>(RankedChatDescriptor {
                    date_millis,
                    descriptor,
                })
            });
        }

        while let Some(joined) = jobs.join_next().await {
            let ranked = joined.map_err(|error| {
                DomainError::InternalError(format!("Recent chat stats scanner failed: {}", error))
            })??;
            ranked_non_pinned.push(ranked);
        }

        ranked_non_pinned.sort_by(compare_ranked_chat_descriptors);

        let mut selected = pinned;
        selected.extend(
            ranked_non_pinned
                .into_iter()
                .take(non_pinned_limit)
                .map(|entry| entry.descriptor),
        );

        Ok(selected)
    }
}
