use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use serde::Deserialize;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_repository::{
    ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchHit, ChatMessageSearchQuery,
};

use super::FileChatRepository;

const MAX_QUERY_TOKENS: usize = 64;
const MAX_BIGRAM_TOKENS_PER_SEGMENT: usize = 32;
const SEARCH_PAGE_SIZE: usize = 1000;
const SNIPPET_MAX_CHARS: usize = 200;
const SNIPPET_CONTEXT_BEFORE: usize = 40;

#[derive(Debug, Deserialize)]
struct SearchableChatMessage {
    #[serde(default)]
    is_user: bool,
    #[serde(default)]
    is_system: bool,
    #[serde(default)]
    mes: String,
}

fn role_from_message(message: &SearchableChatMessage) -> ChatMessageRole {
    if message.is_user {
        ChatMessageRole::User
    } else if message.is_system {
        ChatMessageRole::System
    } else {
        ChatMessageRole::Assistant
    }
}

fn normalize_query(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '_' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn bigram_tokens(value: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    if chars.len() == 1 {
        return vec![value.to_string()];
    }

    let total = chars.len() - 1;
    let step = (total + limit - 1) / limit;
    let mut seen = HashSet::new();
    let mut tokens = Vec::new();

    for index in (0..total).step_by(step) {
        let token = format!("{}{}", chars[index], chars[index + 1]);
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }

    tokens
}

fn expand_query_tokens(tokens: Vec<String>) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }

    if tokens.len() == 1 {
        let token = &tokens[0];
        let char_count = token.chars().count();
        if char_count <= 2 {
            return tokens;
        }

        let mut expanded = bigram_tokens(token, MAX_BIGRAM_TOKENS_PER_SEGMENT);
        if char_count <= 8 && expanded.len() < MAX_BIGRAM_TOKENS_PER_SEGMENT {
            expanded.insert(0, token.clone());
        }
        return expanded;
    }

    let mut expanded = Vec::new();
    for token in tokens {
        let char_count = token.chars().count();
        let is_ascii_word = token.chars().any(|ch| ch.is_ascii_alphanumeric());
        if !is_ascii_word && char_count >= 8 {
            expanded.extend(bigram_tokens(&token, 8));
        } else {
            expanded.push(token);
        }
    }

    expanded
}

fn dedup_and_limit_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        if seen.insert(token.clone()) {
            unique.push(token);
        }
    }

    if unique.len() <= MAX_QUERY_TOKENS {
        return unique;
    }

    let step = (unique.len() + MAX_QUERY_TOKENS - 1) / MAX_QUERY_TOKENS;
    unique
        .into_iter()
        .step_by(step)
        .take(MAX_QUERY_TOKENS)
        .collect()
}

fn build_query_tokens(query: &str) -> Vec<String> {
    let normalized = normalize_query(query);
    if normalized.is_empty() {
        return Vec::new();
    }

    let base = normalized
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let expanded = expand_query_tokens(base);
    dedup_and_limit_tokens(expanded)
}

fn token_weight(token: &str) -> usize {
    token.chars().count().min(8).max(1)
}

fn needs_ascii_lowercase(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()))
}

fn score_text(text: &str, tokens: &[String], needs_lowercase: bool) -> (f32, Option<usize>) {
    if text.trim().is_empty() || tokens.is_empty() {
        return (0.0, None);
    }

    let search_text;
    let haystack: &str = if needs_lowercase {
        search_text = text.to_lowercase();
        &search_text
    } else {
        text
    };

    let mut total_weight: usize = 0;
    let mut matched_weight: usize = 0;
    let mut first_match: Option<usize> = None;

    for token in tokens {
        let weight = token_weight(token);
        total_weight += weight;

        if let Some(pos) = haystack.find(token) {
            matched_weight += weight;
            first_match = match first_match {
                Some(existing) => Some(existing.min(pos)),
                None => Some(pos),
            };
        }
    }

    if total_weight == 0 || matched_weight == 0 {
        return (0.0, first_match);
    }

    let score = (matched_weight as f32) / (total_weight as f32);
    (score, first_match)
}

fn snippet_from_text(text: &str, match_byte: Option<usize>) -> String {
    let total_chars = text.chars().count();
    if total_chars <= SNIPPET_MAX_CHARS {
        return text.to_string();
    }

    if let Some(byte_index) = match_byte {
        let prefix_chars = text.get(..byte_index).unwrap_or_default().chars().count();
        let start = prefix_chars.saturating_sub(SNIPPET_CONTEXT_BEFORE);
        let end = (start + SNIPPET_MAX_CHARS).min(total_chars);
        let snippet: String = text
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();

        let mut output = String::new();
        if start > 0 {
            output.push_str("...");
        }
        output.push_str(&snippet);
        if end < total_chars {
            output.push_str("...");
        }
        return output;
    }

    let tail: String = text
        .chars()
        .rev()
        .take(SNIPPET_MAX_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("...{}", tail)
}

#[derive(Debug)]
struct Candidate {
    index: usize,
    score: f32,
    role: ChatMessageRole,
    text: String,
    match_byte: Option<usize>,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.score.to_bits() == other.score.to_bits() && self.index == other.index
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| self.index.cmp(&other.index))
    }
}

fn resolve_effective_range(
    total_count: usize,
    filters: Option<&ChatMessageSearchFilters>,
) -> (usize, usize) {
    let start = filters.and_then(|value| value.start_index).unwrap_or(0);
    let end = filters
        .and_then(|value| value.end_index)
        .unwrap_or_else(|| total_count.saturating_sub(1));
    (start, end.min(total_count.saturating_sub(1)))
}

fn resolve_scan_limit(
    total_count: usize,
    filters: Option<&ChatMessageSearchFilters>,
) -> Result<usize, DomainError> {
    let scan_limit = filters
        .and_then(|value| value.scan_limit)
        .unwrap_or(total_count);
    if scan_limit == 0 {
        return Err(DomainError::InvalidData(
            "scanLimit must be greater than 0".to_string(),
        ));
    }
    Ok(scan_limit.min(total_count))
}

impl FileChatRepository {
    pub(super) async fn search_character_chat_messages_internal(
        &self,
        character_name: &str,
        file_name: &str,
        query: ChatMessageSearchQuery,
    ) -> Result<Vec<ChatMessageSearchHit>, DomainError> {
        let query_text = query.query.trim();
        if query_text.is_empty() {
            return Err(DomainError::InvalidData(
                "query must not be empty".to_string(),
            ));
        }
        if query.limit == 0 {
            return Err(DomainError::InvalidData(
                "limit must be greater than 0".to_string(),
            ));
        }

        let summary = self
            .get_character_chat_summary_internal(character_name, file_name, false)
            .await?;
        let total_count = summary.message_count;
        if total_count == 0 {
            return Ok(Vec::new());
        }

        let tokens = build_query_tokens(query_text);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let needs_lowercase = needs_ascii_lowercase(&tokens);

        let filters = query.filters.as_ref();
        let role_filter = filters.and_then(|value| value.role);
        let (start_index, end_index) = resolve_effective_range(total_count, filters);
        if start_index > end_index {
            return Ok(Vec::new());
        }

        let mut remaining_scan = resolve_scan_limit(total_count, filters)?;

        let mut heap: BinaryHeap<Reverse<Candidate>> = BinaryHeap::new();

        let page_size = SEARCH_PAGE_SIZE.min(remaining_scan);
        let tail = self
            .get_character_payload_tail_lines(character_name, file_name, page_size)
            .await?;

        let mut window_start_index = total_count.saturating_sub(tail.lines.len());

        self.collect_candidates_from_lines(
            &tail.lines,
            window_start_index,
            start_index,
            end_index,
            role_filter,
            &tokens,
            needs_lowercase,
            query.limit,
            &mut heap,
        )?;

        remaining_scan = remaining_scan.saturating_sub(tail.lines.len());

        let mut cursor = tail.cursor;
        let mut has_more_before = tail.has_more_before;

        while remaining_scan > 0 && has_more_before {
            let page_size = SEARCH_PAGE_SIZE.min(remaining_scan);
            let chunk = self
                .get_character_payload_before_lines(character_name, file_name, cursor, page_size)
                .await?;

            cursor = chunk.cursor;
            has_more_before = chunk.has_more_before;
            window_start_index = window_start_index.saturating_sub(chunk.lines.len());

            let chunk_end_index = window_start_index
                .saturating_add(chunk.lines.len())
                .saturating_sub(1);
            if chunk.lines.is_empty() || chunk_end_index < start_index {
                break;
            }

            self.collect_candidates_from_lines(
                &chunk.lines,
                window_start_index,
                start_index,
                end_index,
                role_filter,
                &tokens,
                needs_lowercase,
                query.limit,
                &mut heap,
            )?;

            remaining_scan = remaining_scan.saturating_sub(chunk.lines.len());
        }

        Ok(finalize_candidates(heap))
    }

    pub(super) async fn search_group_chat_messages_internal(
        &self,
        chat_id: &str,
        query: ChatMessageSearchQuery,
    ) -> Result<Vec<ChatMessageSearchHit>, DomainError> {
        let query_text = query.query.trim();
        if query_text.is_empty() {
            return Err(DomainError::InvalidData(
                "query must not be empty".to_string(),
            ));
        }
        if query.limit == 0 {
            return Err(DomainError::InvalidData(
                "limit must be greater than 0".to_string(),
            ));
        }

        let summary = self.get_group_chat_summary_internal(chat_id, false).await?;
        let total_count = summary.message_count;
        if total_count == 0 {
            return Ok(Vec::new());
        }

        let tokens = build_query_tokens(query_text);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let needs_lowercase = needs_ascii_lowercase(&tokens);

        let filters = query.filters.as_ref();
        let role_filter = filters.and_then(|value| value.role);
        let (start_index, end_index) = resolve_effective_range(total_count, filters);
        if start_index > end_index {
            return Ok(Vec::new());
        }

        let mut remaining_scan = resolve_scan_limit(total_count, filters)?;

        let mut heap: BinaryHeap<Reverse<Candidate>> = BinaryHeap::new();

        let page_size = SEARCH_PAGE_SIZE.min(remaining_scan);
        let tail = self
            .get_group_payload_tail_lines(chat_id, page_size)
            .await?;

        let mut window_start_index = total_count.saturating_sub(tail.lines.len());

        self.collect_candidates_from_lines(
            &tail.lines,
            window_start_index,
            start_index,
            end_index,
            role_filter,
            &tokens,
            needs_lowercase,
            query.limit,
            &mut heap,
        )?;

        remaining_scan = remaining_scan.saturating_sub(tail.lines.len());

        let mut cursor = tail.cursor;
        let mut has_more_before = tail.has_more_before;

        while remaining_scan > 0 && has_more_before {
            let page_size = SEARCH_PAGE_SIZE.min(remaining_scan);
            let chunk = self
                .get_group_payload_before_lines(chat_id, cursor, page_size)
                .await?;

            cursor = chunk.cursor;
            has_more_before = chunk.has_more_before;
            window_start_index = window_start_index.saturating_sub(chunk.lines.len());

            let chunk_end_index = window_start_index
                .saturating_add(chunk.lines.len())
                .saturating_sub(1);
            if chunk.lines.is_empty() || chunk_end_index < start_index {
                break;
            }

            self.collect_candidates_from_lines(
                &chunk.lines,
                window_start_index,
                start_index,
                end_index,
                role_filter,
                &tokens,
                needs_lowercase,
                query.limit,
                &mut heap,
            )?;

            remaining_scan = remaining_scan.saturating_sub(chunk.lines.len());
        }

        Ok(finalize_candidates(heap))
    }

    fn collect_candidates_from_lines(
        &self,
        lines: &[String],
        start_abs_index: usize,
        min_index: usize,
        max_index: usize,
        role_filter: Option<ChatMessageRole>,
        tokens: &[String],
        needs_lowercase: bool,
        limit: usize,
        heap: &mut BinaryHeap<Reverse<Candidate>>,
    ) -> Result<(), DomainError> {
        for (offset, line) in lines.iter().enumerate() {
            let index = start_abs_index.saturating_add(offset);
            if index < min_index || index > max_index {
                continue;
            }

            let message: SearchableChatMessage = serde_json::from_str(line).map_err(|error| {
                DomainError::InvalidData(format!("Failed to parse chat message JSON: {}", error))
            })?;

            let role = role_from_message(&message);
            if let Some(filter) = role_filter {
                if role != filter {
                    continue;
                }
            }

            let (score, match_byte) = score_text(&message.mes, tokens, needs_lowercase);
            if score <= 0.0 {
                continue;
            }

            let candidate = Candidate {
                index,
                score,
                role,
                text: message.mes,
                match_byte,
            };

            if heap.len() < limit {
                heap.push(Reverse(candidate));
                continue;
            }

            let should_insert = heap.peek().map(|entry| candidate > entry.0).unwrap_or(true);
            if should_insert {
                heap.pop();
                heap.push(Reverse(candidate));
            }
        }

        Ok(())
    }
}

fn finalize_candidates(heap: BinaryHeap<Reverse<Candidate>>) -> Vec<ChatMessageSearchHit> {
    let mut candidates = heap.into_iter().map(|entry| entry.0).collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.cmp(a));
    candidates
        .into_iter()
        .map(|candidate| ChatMessageSearchHit {
            index: candidate.index,
            score: candidate.score,
            snippet: snippet_from_text(&candidate.text, candidate.match_byte),
            role: candidate.role,
            text: candidate.text,
        })
        .collect()
}
