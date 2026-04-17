const CLAUDE_FULL_SAMPLING_EXACT_MODELS: &[&str] = &["claude-opus-4", "claude-sonnet-4"];
const CLAUDE_FULL_SAMPLING_MODEL_PREFIXES: &[&str] = &[
    "claude-3-7",
    "claude-3-5",
    "claude-3-opus",
    "claude-3-sonnet",
    "claude-3-haiku",
    "claude-2",
    "claude-instant",
];
const CLAUDE_LIMITED_SAMPLING_MODEL_PREFIXES: &[&str] = &[
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
];
const CLAUDE_MANUAL_ONLY_THINKING_EXACT_MODELS: &[&str] = &["claude-opus-4", "claude-sonnet-4"];
const CLAUDE_MANUAL_ONLY_THINKING_MODEL_PREFIXES: &[&str] = &[
    "claude-3-7",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
];
const CLAUDE_MANUAL_OR_ADAPTIVE_THINKING_MODEL_PREFIXES: &[&str] =
    &["claude-opus-4-6", "claude-sonnet-4-6"];
const CLAUDE_ADAPTIVE_ONLY_THINKING_MODEL_PREFIXES: &[&str] = &["claude-opus-4-7"];
const CLAUDE_OUTPUT_EFFORT_MODEL_PREFIXES: &[&str] = &[
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-opus-4-5",
];
const CLAUDE_ASSISTANT_PREFILL_EXACT_MODELS: &[&str] = &["claude-opus-4", "claude-sonnet-4"];
const CLAUDE_ASSISTANT_PREFILL_MODEL_PREFIXES: &[&str] = &[
    "claude-3-7",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
    "claude-3-5",
    "claude-3-opus",
    "claude-3-sonnet",
    "claude-3-haiku",
    "claude-2",
    "claude-instant",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaudeSamplingMode {
    Full,
    TemperatureOrTopP,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaudeThinkingMode {
    Unsupported,
    ManualOnly,
    ManualOrAdaptive,
    AdaptiveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClaudeModelContract {
    pub(super) sampling: ClaudeSamplingMode,
    pub(super) thinking: ClaudeThinkingMode,
    pub(super) supports_output_effort: bool,
    pub(super) supports_assistant_prefill: bool,
}

impl ClaudeModelContract {
    pub(super) fn resolve(model: &str) -> Self {
        let model = model.trim().to_ascii_lowercase();

        Self {
            sampling: resolve_claude_sampling_mode(&model),
            thinking: resolve_claude_thinking_mode(&model),
            supports_output_effort: matches_claude_model(
                &model,
                &[],
                CLAUDE_OUTPUT_EFFORT_MODEL_PREFIXES,
            ),
            supports_assistant_prefill: matches_claude_model(
                &model,
                CLAUDE_ASSISTANT_PREFILL_EXACT_MODELS,
                CLAUDE_ASSISTANT_PREFILL_MODEL_PREFIXES,
            ),
        }
    }
}

fn resolve_claude_sampling_mode(model: &str) -> ClaudeSamplingMode {
    if matches_claude_model(model, &[], CLAUDE_LIMITED_SAMPLING_MODEL_PREFIXES) {
        ClaudeSamplingMode::TemperatureOrTopP
    } else if matches_claude_model(
        model,
        CLAUDE_FULL_SAMPLING_EXACT_MODELS,
        CLAUDE_FULL_SAMPLING_MODEL_PREFIXES,
    ) {
        ClaudeSamplingMode::Full
    } else {
        ClaudeSamplingMode::None
    }
}

fn resolve_claude_thinking_mode(model: &str) -> ClaudeThinkingMode {
    if matches_claude_model(model, &[], CLAUDE_ADAPTIVE_ONLY_THINKING_MODEL_PREFIXES) {
        ClaudeThinkingMode::AdaptiveOnly
    } else if matches_claude_model(
        model,
        &[],
        CLAUDE_MANUAL_OR_ADAPTIVE_THINKING_MODEL_PREFIXES,
    ) {
        ClaudeThinkingMode::ManualOrAdaptive
    } else if matches_claude_model(
        model,
        CLAUDE_MANUAL_ONLY_THINKING_EXACT_MODELS,
        CLAUDE_MANUAL_ONLY_THINKING_MODEL_PREFIXES,
    ) {
        ClaudeThinkingMode::ManualOnly
    } else {
        ClaudeThinkingMode::Unsupported
    }
}

fn matches_claude_model(model: &str, exact_models: &[&str], prefixes: &[&str]) -> bool {
    exact_models.contains(&model) || prefixes.iter().any(|prefix| model.starts_with(prefix))
}
