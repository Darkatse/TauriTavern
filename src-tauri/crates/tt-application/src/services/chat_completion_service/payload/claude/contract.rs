#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaudeThinkingMode {
    Unsupported,
    Manual,
    Adaptive,
}

/// Only the model differences needed to build the native request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClaudeModelContract {
    pub(super) supports_sampling: bool,
    pub(super) thinking: ClaudeThinkingMode,
    pub(super) supports_output_effort: bool,
    pub(super) supports_xhigh_output_effort: bool,
    pub(super) thinking_defaults_on: bool,
    pub(super) supports_assistant_prefill: bool,
    pub(super) supports_json_output: bool,
}

impl ClaudeModelContract {
    pub(super) fn resolve(model: &str) -> Self {
        let model = model.trim().to_ascii_lowercase();
        let matches = |prefixes: &[&str]| prefixes.iter().any(|prefix| model.starts_with(prefix));
        let claude4 = matches!(model.as_str(), "claude-opus-4" | "claude-sonnet-4");
        let legacy = claude4
            || matches(&[
                "claude-3-7",
                "claude-3-5",
                "claude-3-opus",
                "claude-3-sonnet",
                "claude-3-haiku",
                "claude-2",
                "claude-instant",
            ]);
        let claude45 = matches(&["claude-opus-4-5", "claude-sonnet-4-5", "claude-haiku-4-5"]);
        let claude46 = matches(&["claude-opus-4-6", "claude-sonnet-4-6"]);
        let claude5 = matches(&[
            "claude-opus-5",
            "claude-fable-5",
            "claude-mythos-5",
            "claude-sonnet-5",
            "claude-haiku-5",
        ]);
        let adaptive_only = claude5 || matches(&["claude-opus-4-7", "claude-opus-4-8"]);

        Self {
            supports_sampling: legacy || claude45 || claude46,
            thinking: if adaptive_only || claude46 {
                ClaudeThinkingMode::Adaptive
            } else if claude4 || claude45 || model.starts_with("claude-3-7") {
                ClaudeThinkingMode::Manual
            } else {
                ClaudeThinkingMode::Unsupported
            },
            supports_output_effort: adaptive_only
                || claude46
                || model.starts_with("claude-opus-4-5"),
            supports_xhigh_output_effort: adaptive_only,
            thinking_defaults_on: claude5,
            supports_assistant_prefill: legacy || claude45,
            supports_json_output: model.starts_with("claude-fable-5-1"),
        }
    }
}
