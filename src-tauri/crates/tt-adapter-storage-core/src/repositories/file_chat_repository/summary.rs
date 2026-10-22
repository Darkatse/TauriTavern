use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tt_domain::errors::DomainError;
use tt_domain::models::chat::parse_message_timestamp_value;
use tt_ports::repositories::chat_repository::ChatSearchResult;

use super::FileChatRepository;

mod catalog;
mod index;
mod projection;
mod search;
mod stats;

pub(super) use self::index::{SummaryCache, SummaryCacheEntry};
use self::projection::{FileProjection, MessageText};
use self::search::SearchFingerprint;

/// Persisted directory projection; complete metadata belongs only to a requested response.
/// Changes to its fields or meaning must bump [`index::SCHEMA_VERSION`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ChatSummary {
    pub character_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub message_count: usize,
    pub preview: String,
    pub date: i64,
    pub chat_id: Option<String>,
}

impl From<ChatSummary> for ChatSearchResult {
    fn from(summary: ChatSummary) -> Self {
        Self {
            character_name: summary.character_name,
            file_name: summary.file_name,
            file_size: summary.file_size,
            message_count: summary.message_count,
            preview: summary.preview,
            date: summary.date,
            chat_id: summary.chat_id,
            chat_metadata: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct FileSignature {
    pub size: u64,
    pub modified_millis: i64,
}

struct ScannedSummary {
    projection: FileProjection,
    fingerprint: Option<SearchFingerprint>,
}

#[derive(Clone, Debug)]
pub(super) struct ChatFileDescriptor {
    pub character_name: String,
    pub file_name: String,
    pub path: PathBuf,
}

fn summary_cache_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn directory_date(send_date: Option<&Value>, modified_millis: i64) -> i64 {
    let parsed = parse_message_timestamp_value(send_date);
    if parsed > 0 { parsed } else { modified_millis }
}

fn message_display_text(message: MessageText) -> Option<String> {
    match message {
        MessageText::Missing => None,
        MessageText::Text(text) => Some(text),
        MessageText::Unavailable => Some("Preview unavailable".to_string()),
    }
}

impl FileChatRepository {
    pub(super) fn file_signature_from_metadata(metadata: &std::fs::Metadata) -> FileSignature {
        let modified_millis = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        FileSignature {
            size: metadata.len(),
            modified_millis,
        }
    }

    pub(super) async fn scan_chat_summary_file(
        &self,
        path: &Path,
        fallback_character_name: &str,
        fallback_file_name: &str,
        signature: FileSignature,
        include_fingerprint: bool,
    ) -> Result<(SummaryCacheEntry, Option<String>), DomainError> {
        let scan = if include_fingerprint {
            search::scan_with_fingerprint(path, fallback_file_name).await?
        } else {
            ScannedSummary {
                projection: projection::scan_file(path).await?,
                fingerprint: None,
            }
        };

        let FileProjection {
            line_count,
            header,
            tail,
        } = scan.projection;

        let character_name = header
            .character_name
            .as_deref()
            .filter(|name| {
                let trimmed = name.trim();
                !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unused")
            })
            .unwrap_or(fallback_character_name)
            .to_string();
        let chat_id = header.chat_id;
        let date = directory_date(tail.send_date.as_ref(), signature.modified_millis);
        let preview_unavailable = matches!(&tail.mes, MessageText::Unavailable);
        let last_message = message_display_text(tail.mes);

        let entry = SummaryCacheEntry {
            signature,
            summary: ChatSummary {
                character_name,
                file_name: Self::normalize_jsonl_file_name(fallback_file_name)?,
                file_size: signature.size,
                message_count: line_count.saturating_sub(1),
                preview: last_message
                    .as_deref()
                    .map(preview_message_text)
                    .unwrap_or_default(),
                date,
                chat_id,
            },
            preview_unavailable,
            fingerprint: scan.fingerprint,
        };
        // Full text belongs to this read, never to the bounded preview cache.
        Ok((entry, last_message))
    }
}

fn preview_message_text(message: &str) -> String {
    const MAX_CHARS: usize = 400;

    let Some((index, character)) = message.char_indices().rev().nth(MAX_CHARS) else {
        return message.to_string();
    };
    format!("...{}", &message[index + character.len_utf8()..])
}
