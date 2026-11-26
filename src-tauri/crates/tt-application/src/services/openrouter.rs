use std::collections::HashMap;

pub(crate) fn attribution_headers() -> HashMap<String, String> {
    HashMap::from(
        [
            ("HTTP-Referer", "https://tauritavern.github.io"),
            ("X-OpenRouter-Title", "TauriTavern"),
            ("X-Title", "TauriTavern"),
            ("X-OpenRouter-Categories", "roleplay,general-chat"),
        ]
        .map(|(name, value)| (name.to_string(), value.to_string())),
    )
}
