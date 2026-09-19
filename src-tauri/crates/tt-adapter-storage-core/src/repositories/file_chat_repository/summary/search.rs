use std::hash::{Hash, Hasher};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tt_domain::errors::DomainError;
use tt_domain::models::chat::strip_jsonl_extension;
use tt_ports::repositories::chat_repository::ChatSearchResult;

use super::super::FileChatRepository;
use super::projection::{HeaderProjection, TailProjection};
use super::{ChatFileDescriptor, ScannedSummary};

const FINGERPRINT_WORDS: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct SearchFingerprint {
    bits: Vec<u64>,
}

impl SearchFingerprint {
    pub(super) fn new() -> Self {
        Self {
            bits: vec![0; FINGERPRINT_WORDS],
        }
    }

    pub(super) fn normalize_len(&mut self) {
        if self.bits.len() != FINGERPRINT_WORDS {
            self.bits.resize(FINGERPRINT_WORDS, 0);
        }
    }

    fn bit(hash: u64) -> (usize, u64) {
        let index = (hash % (FINGERPRINT_WORDS as u64 * 64)) as usize;
        (index / 64, 1 << (index % 64))
    }

    fn set_hashed(&mut self, hash: u64) {
        let (word, bit) = Self::bit(hash);
        self.bits[word] |= bit;
    }

    fn has_hashed(&self, hash: u64) -> bool {
        let (word, bit) = Self::bit(hash);
        self.bits.get(word).is_some_and(|value| value & bit != 0)
    }

    fn hash_trigram(chars: [char; 3]) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (chars[0] as u32).hash(&mut hasher);
        (chars[1] as u32).hash(&mut hasher);
        (chars[2] as u32).hash(&mut hasher);
        hasher.finish()
    }

    fn visit_trigram_hashes(value: &str, mut visit: impl FnMut(u64)) -> bool {
        let mut chars = value.chars().flat_map(char::to_lowercase);
        let (Some(mut first), Some(mut second), Some(mut third)) =
            (chars.next(), chars.next(), chars.next())
        else {
            return false;
        };

        loop {
            visit(Self::hash_trigram([first, second, third]));
            let Some(next) = chars.next() else {
                return true;
            };
            (first, second, third) = (second, third, next);
        }
    }

    pub(super) fn add_text(&mut self, value: &str) {
        self.normalize_len();
        Self::visit_trigram_hashes(value, |hash| self.set_hashed(hash));
    }

    fn might_match_fragment(&self, fragment: &str) -> bool {
        if fragment.chars().count() < 3 {
            return true;
        }
        let mut matches = true;
        let saw_trigram = Self::visit_trigram_hashes(fragment, |hash| {
            matches &= self.has_hashed(hash);
        });
        !saw_trigram || matches
    }

    pub(super) fn might_match_fragments(&self, fragments: &[String]) -> bool {
        fragments
            .iter()
            .all(|fragment| self.might_match_fragment(fragment))
    }
}

impl FileChatRepository {
    pub(in crate::repositories::file_chat_repository) async fn collect_matching_chat_summaries(
        &self,
        descriptors: Vec<ChatFileDescriptor>,
        fragments: &[String],
    ) -> (Vec<ChatSearchResult>, bool) {
        let mut results = Vec::new();
        let mut complete = true;

        for descriptor in descriptors {
            let entry = match self.get_chat_summary_entry(&descriptor, true).await {
                Ok(entry) => entry,
                Err(error) => {
                    complete = false;
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Failed to inspect chat '{}': {}",
                        descriptor.path.display(),
                        error
                    );
                    continue;
                }
            };
            let mut summary = entry.summary.clone();
            summary.chat_metadata = None;
            let file_stem = strip_jsonl_extension(&descriptor.file_name);

            if Self::file_stem_matches_all(file_stem, fragments) {
                results.push(summary);
                continue;
            }
            if !entry
                .fingerprint
                .as_ref()
                .expect("fingerprint is required for search")
                .might_match_fragments(fragments)
            {
                continue;
            }

            match self
                .file_matches_query(&descriptor.path, file_stem, fragments)
                .await
            {
                Ok(true) => results.push(summary),
                Ok(false) => {}
                Err(error) => {
                    complete = false;
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Failed to search chat '{}': {}",
                        descriptor.path.display(),
                        error
                    );
                }
            }
        }
        (results, complete)
    }

    pub(in crate::repositories::file_chat_repository) fn file_stem_matches_all(
        file_stem: &str,
        fragments: &[String],
    ) -> bool {
        let lowered = file_stem.to_lowercase();
        fragments.iter().all(|fragment| lowered.contains(fragment))
    }

    pub(in crate::repositories::file_chat_repository) async fn file_matches_query(
        &self,
        path: &Path,
        file_stem: &str,
        fragments: &[String],
    ) -> Result<bool, DomainError> {
        if fragments.is_empty() {
            return Ok(true);
        }

        let mut matches = vec![false; fragments.len()];
        let file_stem = file_stem.to_lowercase();
        for (index, fragment) in fragments.iter().enumerate() {
            matches[index] = file_stem.contains(fragment);
        }
        if matches.iter().all(|matched| *matched) {
            return Ok(true);
        }

        let file = File::open(path).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to open chat file {:?}: {error}", path))
        })?;
        let mut lines = BufReader::new(file).lines();
        while let Some(line) = lines.next_line().await.map_err(|error| {
            DomainError::InternalError(format!("Failed to read chat file {:?}: {error}", path))
        })? {
            if line.trim().is_empty() {
                continue;
            }
            let line = line.to_lowercase();
            for (index, fragment) in fragments.iter().enumerate() {
                matches[index] |= line.contains(fragment);
            }
            if matches.iter().all(|matched| *matched) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(in crate::repositories::file_chat_repository) fn normalize_search_query(
        query: &str,
    ) -> String {
        query
            .trim()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(in crate::repositories::file_chat_repository) fn search_fragments(
        query: &str,
    ) -> Vec<String> {
        query
            .trim()
            .to_lowercase()
            .split_whitespace()
            .map(ToString::to_string)
            .collect()
    }
}

pub(super) async fn scan_with_fingerprint(
    path: &Path,
    fallback_file_name: &str,
) -> Result<ScannedSummary, DomainError> {
    let file = File::open(path).await.map_err(|error| {
        DomainError::InternalError(format!("Failed to open chat file {:?}: {error}", path))
    })?;
    let mut lines = BufReader::new(file).lines();
    let mut line_count = 0;
    let mut header = None;
    let mut last_line = String::new();
    let mut fingerprint = SearchFingerprint::new();
    fingerprint.add_text(strip_jsonl_extension(fallback_file_name));

    while let Some(line) = lines.next_line().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to read chat file {:?}: {error}", path))
    })? {
        if line.trim().is_empty() {
            continue;
        }
        if header.is_none() {
            header = Some(parse_record(path, "first non-empty chat record", &line)?);
        }
        line_count += 1;
        fingerprint.add_text(&line);
        last_line.clear();
        last_line.push_str(&line);
    }

    let header: HeaderProjection = header.unwrap_or_default();
    let tail: TailProjection = if line_count == 0 {
        TailProjection::default()
    } else {
        parse_record(path, "last non-empty chat record", &last_line)?
    };
    Ok(ScannedSummary {
        line_count,
        character_name: header.character_name,
        chat_metadata: header.chat_metadata,
        last_message: tail.mes,
        send_date: tail.send_date,
        fingerprint: Some(fingerprint),
    })
}

fn parse_record<T: DeserializeOwned>(
    path: &Path,
    record_name: &str,
    line: &str,
) -> Result<T, DomainError> {
    serde_json::from_str(line).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to parse {record_name} in {}: {error}",
            path.display()
        ))
    })
}
