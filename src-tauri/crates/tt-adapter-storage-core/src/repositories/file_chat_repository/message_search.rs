use serde::Deserialize;

use tt_domain::errors::DomainError;
use tt_domain::text_search::RankedTextSearch;
use tt_ports::repositories::chat_repository::{
    ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchHit, ChatMessageSearchQuery,
};

use super::{FileChatRepository, classify_message_role};
use crate::chat_jsonl::trim_whitespace;

const SEARCH_PAGE_SIZE: usize = 1000;

#[derive(Debug, Deserialize)]
struct SearchableChatMessage {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    is_user: bool,
    #[serde(default)]
    is_system: bool,
    #[serde(default)]
    mes: String,
}

fn role_from_message(message: &SearchableChatMessage) -> ChatMessageRole {
    classify_message_role(message.role.as_deref(), message.is_user, message.is_system)
}

struct CandidateSearchPlan {
    min_index: usize,
    max_index: usize,
    role_filter: Option<ChatMessageRole>,
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

        let mut search = RankedTextSearch::new(query_text, query.limit);
        if search.is_empty() {
            return Ok(Vec::new());
        }

        let filters = query.filters.as_ref();
        let role_filter = filters.and_then(|value| value.role);
        let (start_index, end_index) = resolve_effective_range(total_count, filters);
        if start_index > end_index {
            return Ok(Vec::new());
        }

        let mut remaining_scan = resolve_scan_limit(total_count, filters)?;
        let plan = CandidateSearchPlan {
            min_index: start_index,
            max_index: end_index,
            role_filter,
        };

        let page_size = SEARCH_PAGE_SIZE.min(remaining_scan);
        let tail = self
            .get_character_payload_tail_lines(character_name, file_name, page_size)
            .await?;

        let mut window_start_index = total_count.saturating_sub(tail.lines.len());

        collect_candidates_from_lines(&tail.lines, window_start_index, &plan, &mut search)?;

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
            if chunk.lines.is_empty() || chunk_end_index < plan.min_index {
                break;
            }

            collect_candidates_from_lines(&chunk.lines, window_start_index, &plan, &mut search)?;

            remaining_scan = remaining_scan.saturating_sub(chunk.lines.len());
        }

        Ok(finalize_candidates(search))
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

        let mut search = RankedTextSearch::new(query_text, query.limit);
        if search.is_empty() {
            return Ok(Vec::new());
        }

        let filters = query.filters.as_ref();
        let role_filter = filters.and_then(|value| value.role);
        let (start_index, end_index) = resolve_effective_range(total_count, filters);
        if start_index > end_index {
            return Ok(Vec::new());
        }

        let mut remaining_scan = resolve_scan_limit(total_count, filters)?;
        let plan = CandidateSearchPlan {
            min_index: start_index,
            max_index: end_index,
            role_filter,
        };

        let page_size = SEARCH_PAGE_SIZE.min(remaining_scan);
        let tail = self
            .get_group_payload_tail_lines(chat_id, page_size)
            .await?;

        let mut window_start_index = total_count.saturating_sub(tail.lines.len());

        collect_candidates_from_lines(&tail.lines, window_start_index, &plan, &mut search)?;

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
            if chunk.lines.is_empty() || chunk_end_index < plan.min_index {
                break;
            }

            collect_candidates_from_lines(&chunk.lines, window_start_index, &plan, &mut search)?;

            remaining_scan = remaining_scan.saturating_sub(chunk.lines.len());
        }

        Ok(finalize_candidates(search))
    }
}

fn collect_candidates_from_lines(
    lines: &[String],
    start_abs_index: usize,
    plan: &CandidateSearchPlan,
    search: &mut RankedTextSearch<'static, ChatMessageRole>,
) -> Result<(), DomainError> {
    for (offset, line) in lines.iter().enumerate() {
        let index = start_abs_index.saturating_add(offset);
        if index < plan.min_index || index > plan.max_index {
            continue;
        }

        if !trim_whitespace(line.as_bytes()).starts_with(b"{") {
            return Err(DomainError::InvalidData(format!(
                "Chat message {index} must be a JSON object"
            )));
        }
        let message: SearchableChatMessage = serde_json::from_str(line).map_err(|error| {
            DomainError::InvalidData(format!("Failed to parse chat message JSON: {}", error))
        })?;

        let role = role_from_message(&message);
        if let Some(filter) = plan.role_filter
            && role != filter
        {
            continue;
        }
        search.offer(index, message.mes, role);
    }

    Ok(())
}

fn finalize_candidates(
    search: RankedTextSearch<'static, ChatMessageRole>,
) -> Vec<ChatMessageSearchHit> {
    search
        .finish()
        .into_iter()
        .map(|hit| ChatMessageSearchHit {
            index: hit.index,
            score: hit.score,
            snippet: hit.snippet,
            role: hit.item,
            text: hit.text.into_owned(),
        })
        .collect()
}
