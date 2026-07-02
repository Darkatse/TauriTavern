use serde::Serialize;
use serde_json::Value;

use tt_domain::text_metrics::TextMetrics;

// AgentToolResult keeps `structured` as a provider-facing JSON boundary; tool
// modules build typed payloads and cross that boundary only here.
pub(in crate::services::agent_tools) fn structured_value<T: Serialize>(payload: T) -> Value {
    serde_json::to_value(payload).expect("agent.tool_structured_payload_serialization_failed")
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::services::agent_tools) struct TextMetricsPayload {
    pub chars: usize,
    pub words: usize,
}

impl From<TextMetrics> for TextMetricsPayload {
    fn from(metrics: TextMetrics) -> Self {
        Self {
            chars: metrics.chars,
            words: metrics.words,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::services::agent_tools) struct TextTotalMetricsPayload {
    pub total_chars: usize,
    pub total_words: usize,
}

impl From<TextMetrics> for TextTotalMetricsPayload {
    fn from(metrics: TextMetrics) -> Self {
        Self {
            total_chars: metrics.chars,
            total_words: metrics.words,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::services::agent_tools) struct TextSelectionMetricsPayload {
    #[serde(flatten)]
    pub selected: TextMetricsPayload,
    #[serde(flatten)]
    pub total: TextTotalMetricsPayload,
}

impl TextSelectionMetricsPayload {
    pub(in crate::services::agent_tools) fn new(selected: TextMetrics, total: TextMetrics) -> Self {
        Self {
            selected: selected.into(),
            total: total.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::services::agent_tools) struct TextRangeMetricsPayload {
    #[serde(flatten)]
    pub metrics: TextSelectionMetricsPayload,
    pub start_char: usize,
    pub end_char: usize,
    /// True when the returned text does not cover the full source text.
    pub truncated: bool,
}

impl TextRangeMetricsPayload {
    pub(in crate::services::agent_tools) fn new(
        selected: TextMetrics,
        total: TextMetrics,
        start_char: usize,
        end_char: usize,
    ) -> Self {
        assert!(
            start_char <= end_char && end_char <= total.chars,
            "agent.tool_text_range_offsets_invalid"
        );
        Self {
            metrics: TextSelectionMetricsPayload::new(selected, total),
            start_char,
            end_char,
            truncated: start_char > 0 || end_char < total.chars,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::services::agent_tools) struct ToolErrorStructured<'a> {
    pub error: ToolErrorBody<'a>,
}

impl<'a> ToolErrorStructured<'a> {
    pub(in crate::services::agent_tools) fn new(code: &'a str, message: &'a str) -> Self {
        Self {
            error: ToolErrorBody { code, message },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::services::agent_tools) struct ToolErrorBody<'a> {
    pub code: &'a str,
    pub message: &'a str,
}
