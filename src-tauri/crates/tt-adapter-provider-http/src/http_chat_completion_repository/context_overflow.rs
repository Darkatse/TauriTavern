use std::sync::LazyLock;

use regex::{RegexSet, RegexSetBuilder};

// Adapted from pi-ai 1.3.0 utils/overflow.js (MIT); transport body limits are not context limits.
pub(super) fn is_context_overflow(text: &str) -> bool {
    static OVERFLOW: LazyLock<RegexSet> = LazyLock::new(|| {
        RegexSetBuilder::new([
            r"prompt (?:is )?too long",
            r"prompt exceeds max length",
            r"input is too long for requested model",
            r"input length and `?max_tokens`? exceed context limit",
            r"exceeds the context window",
            r"exceeds (?:the )?(?:model'?s )?maximum context length(?: of [\d,]+ tokens?|\s*\([\d,]+\))",
            r"input token count.*exceeds the maximum",
            r"maximum prompt length is \d+",
            r"reduce the length of the messages",
            r"maximum context length is \d+ tokens",
            r"exceeds (?:the )?maximum allowed input length of [\d,]+ tokens?",
            r"input \(\d+ tokens\) is longer than the model'?s context length \(\d+ tokens\)",
            r"exceeds the limit of \d+",
            r"exceeds the available context size",
            r"greater than the context length",
            r"context window exceeds limit",
            r"exceeded model token limit",
            r"too large for model with \d+ maximum context length",
            r"prompt has [\d,]+ tokens?, but the configured context size is [\d,]+ tokens?",
            r"model_context_window_exceeded",
            r"prompt too long; exceeded (?:max )?context length",
            r"range of input length should be",
            r"context[_ ]length[_ ]exceeded",
            r"too many tokens",
            r"token limit exceeded",
        ])
        .case_insensitive(true)
        .build()
        .expect("context overflow patterns are valid")
    });
    static EXCLUDED: LazyLock<RegexSet> = LazyLock::new(|| {
        RegexSetBuilder::new([r"rate limit", r"too many requests"])
            .case_insensitive(true)
            .build()
            .expect("context overflow exclusion patterns are valid")
    });

    !EXCLUDED.is_match(text) && OVERFLOW.is_match(text)
}
