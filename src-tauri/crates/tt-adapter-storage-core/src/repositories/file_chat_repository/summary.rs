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
use self::search::SearchFingerprint;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct FileSignature {
    pub size: u64,
    pub modified_millis: i64,
}

struct ScannedSummary {
    line_count: usize,
    character_name: Option<String>,
    chat_metadata: Option<Value>,
    last_message: Option<String>,
    send_date: Option<Value>,
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
    ) -> Result<SummaryCacheEntry, DomainError> {
        let scan = if include_fingerprint {
            search::scan_with_fingerprint(path, fallback_file_name).await?
        } else {
            let projected = projection::scan_file(path).await?;
            ScannedSummary {
                line_count: projected.line_count,
                character_name: projected.header.character_name,
                chat_metadata: projected.header.chat_metadata,
                last_message: projected.tail.mes,
                send_date: projected.tail.send_date,
                fingerprint: None,
            }
        };

        let character_name = scan
            .character_name
            .as_deref()
            .filter(|name| {
                let trimmed = name.trim();
                !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unused")
            })
            .unwrap_or(fallback_character_name)
            .to_string();
        let chat_id = scan
            .chat_metadata
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("chat_id_hash"))
            .and_then(|value| {
                value
                    .as_u64()
                    .map(|number| number.to_string())
                    .or_else(|| value.as_i64().map(|number| number.to_string()))
                    .or_else(|| value.as_str().map(ToString::to_string))
            });
        let parsed_date = parse_message_timestamp_value(scan.send_date.as_ref());

        Ok(SummaryCacheEntry {
            signature,
            summary: ChatSearchResult {
                character_name,
                file_name: Self::normalize_jsonl_file_name(fallback_file_name)?,
                file_size: signature.size,
                message_count: scan.line_count.saturating_sub(1),
                preview: scan
                    .last_message
                    .as_deref()
                    .map(preview_message_text)
                    .unwrap_or_default(),
                date: if parsed_date > 0 {
                    parsed_date
                } else {
                    signature.modified_millis
                },
                chat_id,
                chat_metadata: scan.chat_metadata,
            },
            fingerprint: scan.fingerprint,
        })
    }
}

fn preview_message_text(message: &str) -> String {
    const MAX_CHARS: usize = 400;

    let Some((index, character)) = message.char_indices().rev().nth(MAX_CHARS) else {
        return message.to_string();
    };
    format!("...{}", &message[index + character.len_utf8()..])
}
