use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::AgentRuntimeService;
use crate::application::dto::agent_dto::{AgentSubmitGuidanceDto, AgentSubmitGuidanceResultDto};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole,
    AgentRunEventLevel, AgentRunStatus, ROOT_AGENT_INVOCATION_ID,
};
use crate::domain::text_metrics::TextMetrics;

const MAX_GUIDANCE_TEXT_CHARS: usize = 16_000;
const MAX_PENDING_GUIDANCE_ITEMS: usize = 8;
const MAX_PENDING_GUIDANCE_CHARS: usize = 64_000;
const GUIDANCE_PREVIEW_CHARS: usize = 240;
const USER_GUIDANCE_OPEN_TAG: &str = "<user_guidance>";
const USER_GUIDANCE_CLOSE_TAG: &str = "</user_guidance>";

#[derive(Debug, Clone)]
pub(super) struct AgentGuidanceItem {
    pub(super) guidance_id: String,
    pub(super) client_guidance_id: Option<String>,
    pub(super) text: String,
    pub(super) preview: String,
    pub(super) metrics: TextMetrics,
    pub(super) submitted_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub(super) struct AgentGuidanceMailbox {
    state: Mutex<AgentGuidanceMailboxState>,
}

#[derive(Debug, Default)]
struct AgentGuidanceMailboxState {
    queue: VecDeque<AgentGuidanceItem>,
    pending_chars: usize,
    closed: bool,
}

impl AgentGuidanceMailbox {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) async fn ensure_can_accept(&self, chars: usize) -> Result<(), ApplicationError> {
        let state = self.state.lock().await;
        validate_accepts(&state, chars)
    }

    pub(super) async fn enqueue(&self, item: AgentGuidanceItem) -> Result<usize, ApplicationError> {
        let mut state = self.state.lock().await;
        validate_accepts(&state, item.metrics.chars)?;
        state.pending_chars += item.metrics.chars;
        state.queue.push_back(item);
        Ok(state.queue.len())
    }

    pub(super) async fn drain(&self) -> Vec<AgentGuidanceItem> {
        let mut state = self.state.lock().await;
        let items = state.queue.drain(..).collect::<Vec<_>>();
        state.pending_chars = 0;
        items
    }

    pub(super) async fn close_and_drain(&self) -> Vec<AgentGuidanceItem> {
        let mut state = self.state.lock().await;
        state.closed = true;
        let items = state.queue.drain(..).collect::<Vec<_>>();
        state.pending_chars = 0;
        items
    }
}

impl AgentRuntimeService {
    pub async fn submit_guidance(
        &self,
        dto: AgentSubmitGuidanceDto,
    ) -> Result<AgentSubmitGuidanceResultDto, ApplicationError> {
        let run_id = normalize_required("runId", &dto.run_id)?;
        let text = normalize_guidance_text(&dto.text)?;
        let metrics = TextMetrics::from_text(&text);
        if metrics.chars > MAX_GUIDANCE_TEXT_CHARS {
            return Err(ApplicationError::ValidationError(format!(
                "agent.guidance_too_large: guidance is {} chars; maximum is {}",
                metrics.chars, MAX_GUIDANCE_TEXT_CHARS
            )));
        }

        let run = self.run_repository.load_run(run_id.as_str()).await?;
        if !run_status_accepts_guidance(run.status) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.guidance_run_not_accepting: run `{}` is {} and cannot accept guidance",
                run.id,
                serde_json::to_string(&run.status)
                    .unwrap_or_else(|_| "unknown".to_string())
                    .trim_matches('"')
            )));
        }

        let active_handle = self.active_runs.read().await.get(&run.id).cloned();
        let active_handle = active_handle.ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.guidance_active_run_missing: run `{}` is not active",
                run.id
            ))
        })?;

        active_handle
            .guidance_mailbox
            .ensure_can_accept(metrics.chars)
            .await?;

        let item = AgentGuidanceItem {
            guidance_id: format!("guidance_{}", Uuid::new_v4().simple()),
            client_guidance_id: normalize_optional(dto.client_guidance_id.as_deref()),
            preview: guidance_preview(&text),
            text,
            metrics,
            submitted_at: Utc::now(),
        };

        self.event(
            &run.id,
            AgentRunEventLevel::Info,
            "user_guidance_submitted",
            json!({
                "guidanceId": item.guidance_id.as_str(),
                "clientGuidanceId": item.client_guidance_id.as_deref(),
                "invocationId": ROOT_AGENT_INVOCATION_ID,
                "chars": item.metrics.chars,
                "words": item.metrics.words,
                "preview": item.preview.as_str(),
                "text": item.text.as_str(),
                "submittedAt": item.submitted_at,
                "status": "queued",
            }),
        )
        .await?;

        let pending_count = match active_handle.guidance_mailbox.enqueue(item.clone()).await {
            Ok(pending_count) => pending_count,
            Err(error) => {
                self.emit_guidance_discarded(
                    &run.id,
                    &[item],
                    "submit_rejected_after_journal_append",
                    AgentRunEventLevel::Warn,
                )
                .await?;
                return Err(error);
            }
        };

        Ok(AgentSubmitGuidanceResultDto {
            run_id: run.id,
            guidance_id: item.guidance_id,
            client_guidance_id: item.client_guidance_id,
            status: "queued".to_string(),
            preview: item.preview,
            chars: item.metrics.chars,
            words: item.metrics.words,
            pending_count,
        })
    }

    pub(super) async fn apply_pending_guidance_to_request(
        &self,
        run_id: &str,
        invocation_id: &str,
        round: usize,
        request: &mut AgentModelRequest,
    ) -> Result<(), ApplicationError> {
        let items = self.drain_pending_guidance(run_id).await;
        if items.is_empty() {
            return Ok(());
        }

        let message_index = request.messages.len();
        let message_text = build_guidance_message(&items);
        request.messages.push(AgentModelMessage {
            role: AgentModelRole::User,
            parts: vec![AgentModelContentPart::Text { text: message_text }],
            provider_metadata: Value::Null,
        });

        let summary = guidance_batch_summary(&items);
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "user_guidance_applied",
            json!({
                "guidanceIds": summary.guidance_ids,
                "clientGuidanceIds": summary.client_guidance_ids,
                "invocationId": invocation_id,
                "round": round,
                "count": summary.count,
                "chars": summary.chars,
                "words": summary.words,
                "preview": summary.preview,
                "messageIndex": message_index,
                "requestMessageCount": request.messages.len(),
                "status": "applied",
            }),
        )
        .await?;

        Ok(())
    }

    pub(super) async fn close_guidance_mailbox_for_run(
        &self,
        run_id: &str,
        reason: &str,
        level: AgentRunEventLevel,
    ) -> Result<(), ApplicationError> {
        let Some(handle) = self.active_runs.read().await.get(run_id).cloned() else {
            return Ok(());
        };
        let items = handle.guidance_mailbox.close_and_drain().await;
        if items.is_empty() {
            return Ok(());
        }
        self.emit_guidance_discarded(run_id, &items, reason, level)
            .await
    }

    async fn drain_pending_guidance(&self, run_id: &str) -> Vec<AgentGuidanceItem> {
        let Some(handle) = self.active_runs.read().await.get(run_id).cloned() else {
            return Vec::new();
        };
        handle.guidance_mailbox.drain().await
    }

    async fn emit_guidance_discarded(
        &self,
        run_id: &str,
        items: &[AgentGuidanceItem],
        reason: &str,
        level: AgentRunEventLevel,
    ) -> Result<(), ApplicationError> {
        let summary = guidance_batch_summary(items);
        self.event(
            run_id,
            level,
            "user_guidance_discarded",
            json!({
                "guidanceIds": summary.guidance_ids,
                "clientGuidanceIds": summary.client_guidance_ids,
                "invocationId": ROOT_AGENT_INVOCATION_ID,
                "count": summary.count,
                "chars": summary.chars,
                "words": summary.words,
                "preview": summary.preview,
                "reason": reason,
                "status": "discarded",
            }),
        )
        .await?;
        Ok(())
    }
}

fn validate_accepts(
    state: &AgentGuidanceMailboxState,
    incoming_chars: usize,
) -> Result<(), ApplicationError> {
    if state.closed {
        return Err(ApplicationError::ValidationError(
            "agent.guidance_mailbox_closed: run is no longer accepting guidance".to_string(),
        ));
    }
    if state.queue.len() >= MAX_PENDING_GUIDANCE_ITEMS {
        return Err(ApplicationError::ValidationError(format!(
            "agent.guidance_mailbox_full: pending guidance item limit {} reached",
            MAX_PENDING_GUIDANCE_ITEMS
        )));
    }
    if state.pending_chars + incoming_chars > MAX_PENDING_GUIDANCE_CHARS {
        return Err(ApplicationError::ValidationError(format!(
            "agent.guidance_mailbox_full: pending guidance char limit {} exceeded",
            MAX_PENDING_GUIDANCE_CHARS
        )));
    }
    Ok(())
}

fn run_status_accepts_guidance(status: AgentRunStatus) -> bool {
    !matches!(
        status,
        AgentRunStatus::Finishing
            | AgentRunStatus::Completed
            | AgentRunStatus::PartialSuccess
            | AgentRunStatus::Cancelling
            | AgentRunStatus::Cancelled
            | AgentRunStatus::Failed
    )
}

fn normalize_required(field: &str, value: &str) -> Result<String, ApplicationError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApplicationError::ValidationError(format!(
            "agent.guidance_invalid: {field} is required"
        )));
    }
    Ok(value.to_string())
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_guidance_text(value: &str) -> Result<String, ApplicationError> {
    let text = value.trim();
    if text.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.guidance_empty: guidance text cannot be empty".to_string(),
        ));
    }
    Ok(text.to_string())
}

fn guidance_preview(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, GUIDANCE_PREVIEW_CHARS)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect::<String>()
}

fn build_guidance_message(items: &[AgentGuidanceItem]) -> String {
    let mut text = String::from(USER_GUIDANCE_OPEN_TAG);
    text.push_str(
        "\nThe user sent the following guidance while you were working. \
         Apply the guidance in order as the user's latest direction for your next step, \
         within your existing instructions and tool rules.",
    );

    if items.len() == 1 {
        text.push_str("\n\n");
        text.push_str(&items[0].text);
        text.push('\n');
        text.push_str(USER_GUIDANCE_CLOSE_TAG);
        return text;
    }

    for (index, item) in items.iter().enumerate() {
        text.push_str(&format!(
            "\n\n<guidance index=\"{}\">\n{}\n</guidance>",
            index + 1,
            item.text
        ));
    }
    text.push('\n');
    text.push_str(USER_GUIDANCE_CLOSE_TAG);
    text
}

struct GuidanceBatchSummary {
    guidance_ids: Vec<String>,
    client_guidance_ids: Vec<String>,
    count: usize,
    chars: usize,
    words: usize,
    preview: String,
}

fn guidance_batch_summary(items: &[AgentGuidanceItem]) -> GuidanceBatchSummary {
    let guidance_ids = items
        .iter()
        .map(|item| item.guidance_id.clone())
        .collect::<Vec<_>>();
    let client_guidance_ids = items
        .iter()
        .filter_map(|item| item.client_guidance_id.clone())
        .collect::<Vec<_>>();
    let chars = items.iter().map(|item| item.metrics.chars).sum();
    let words = items.iter().map(|item| item.metrics.words).sum();
    let preview = if items.len() == 1 {
        items[0].preview.clone()
    } else {
        guidance_preview(
            &items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    };

    GuidanceBatchSummary {
        guidance_ids,
        client_guidance_ids,
        count: items.len(),
        chars,
        words,
        preview,
    }
}
