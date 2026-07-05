/// Shared Claude model-id helpers that are intentionally provider-agnostic.
///
/// Google Vertex AI exposes Anthropic partner models as `claude-*` ids, but
/// some catalog entries use an `@YYYYMMDD` revision suffix. The application
/// Claude payload builder resolves capabilities from Anthropic-direct prefixes
/// like `claude-sonnet-4-5` / `claude-sonnet-4-5-20250929`, while Vertex URLs
/// use the catalog id with an optional `@YYYYMMDD` revision suffix.
pub fn is_vertex_ai_claude_model_id(model_id: &str) -> bool {
    model_id.trim().to_ascii_lowercase().starts_with("claude-")
}

pub fn normalize_vertex_ai_claude_model_id(model_id: &str) -> String {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = trimmed.to_ascii_lowercase();
    if let Some((base, revision)) = normalized.split_once('@') {
        let revision = revision.trim();
        if revision.is_empty() {
            return base.to_string();
        }
        return format!("{}-{}", base.trim_end_matches('-'), revision);
    }

    normalized
}

pub fn supports_one_hour_prompt_cache(model_id: &str) -> bool {
    let model = normalize_vertex_ai_claude_model_id(model_id);
    !model.starts_with("claude-3-7-sonnet")
        && !model.starts_with("claude-3-5-sonnet")
        && !model.starts_with("claude-3-opus")
}

#[cfg(test)]
mod tests {
    use super::{
        is_vertex_ai_claude_model_id, normalize_vertex_ai_claude_model_id,
        supports_one_hour_prompt_cache,
    };

    #[test]
    fn detects_vertex_claude_ids() {
        assert!(is_vertex_ai_claude_model_id("claude-sonnet-4-5@20250929"));
        assert!(is_vertex_ai_claude_model_id("  Claude-Opus-4-8  "));
        assert!(!is_vertex_ai_claude_model_id("gemini-2.5-pro"));
        assert!(!is_vertex_ai_claude_model_id("anthropic.claude-sonnet-4-5"));
    }

    #[test]
    fn normalizes_vertex_revision_suffix_for_contract_matching() {
        assert_eq!(
            normalize_vertex_ai_claude_model_id("claude-sonnet-4-5@20250929"),
            "claude-sonnet-4-5-20250929"
        );
        assert_eq!(
            normalize_vertex_ai_claude_model_id(" Claude-Opus-4-8 "),
            "claude-opus-4-8"
        );
    }

    #[test]
    fn one_hour_prompt_cache_support_excludes_known_old_series() {
        assert!(!supports_one_hour_prompt_cache(
            "claude-3-7-sonnet@20250219"
        ));
        assert!(!supports_one_hour_prompt_cache(
            "claude-3-5-sonnet-v2@20241022"
        ));
        assert!(!supports_one_hour_prompt_cache("claude-3-opus@20240229"));
        assert!(supports_one_hour_prompt_cache("claude-sonnet-4-5@20250929"));
    }
}
