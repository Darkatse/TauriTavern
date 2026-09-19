use serde::Serialize;
use serde_json::Value;

use tt_domain::text_metrics::TextMetrics;

// AgentToolResult keeps `structured` for runtime state, audit, and timeline UI;
// tool modules build typed payloads and cross that internal boundary only here.
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
pub(in crate::services::agent_tools) struct TextLineRangePayload {
    #[serde(flatten)]
    pub metrics: TextSelectionMetricsPayload,
    pub total_lines: usize,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_start_line: Option<usize>,
    pub line_truncated: bool,
    /// True when the returned text does not cover the full source text.
    pub truncated: bool,
}

impl TextLineRangePayload {
    pub(in crate::services::agent_tools) fn new(
        selected: TextMetrics,
        total: TextMetrics,
        total_lines: usize,
        start_line: usize,
        end_line: usize,
        line_truncated: bool,
    ) -> Self {
        assert!(
            (total_lines == 0 && start_line == 0 && end_line == 0)
                || (start_line >= 1 && start_line <= end_line && end_line <= total_lines),
            "agent.tool_text_line_range_invalid"
        );
        Self {
            metrics: TextSelectionMetricsPayload::new(selected, total),
            total_lines,
            start_line,
            end_line,
            next_start_line: (end_line < total_lines).then_some(end_line + 1),
            line_truncated,
            truncated: line_truncated || start_line > 1 || end_line < total_lines,
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
