use serde_json::json;

use super::*;
use crate::services::llm_connection_service::ResolvedLlmSecretRef;

fn model_binding(
    source: &str,
    model_id: &str,
    custom_api_format: Option<&str>,
) -> ResolvedLlmModelBinding {
    ResolvedLlmModelBinding {
        mode: "connectionRef".to_string(),
        connection_ref: "test-connection".to_string(),
        connection_display_name: "Test Connection".to_string(),
        chat_completion_source: source.to_string(),
        custom_api_format: custom_api_format.map(str::to_string),
        model_id: model_id.to_string(),
        secret_ref: ResolvedLlmSecretRef {
            key: "api_key_deepseek".to_string(),
            id: "secret-1".to_string(),
            label_snapshot: None,
        },
    }
}

#[test]
fn normalizes_frozen_run_input_snapshot() {
    let snapshot = normalize_frozen_run_input_snapshot(
        &json!({
            "schemaVersion": 1,
            "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
            "generationType": "swipe",
            "promptInputs": { "type": "swipe", "messages": [] },
            "worldInfoActivation": { "entries": [] },
            "macroContext": { "names": { "user": "User", "char": "Char" } },
            "variables": { "local": { "score": 42 }, "global": { "theme": "dark" } },
            "currentModelConnection": {
                "schemaVersion": 1,
                "kind": CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
                "settings": {
                    "chat_completion_source": "custom",
                    "model": "opencode-model",
                    "custom_model": "opencode-model",
                    "custom_url": "https://opencode.example.test/v1",
                    "custom_api_format": "openai_compat",
                    "secret_id": "opencode-secret"
                }
            },
        }),
        "swipe",
    )
    .unwrap();

    assert_eq!(snapshot["generationType"], "swipe");
    assert_eq!(snapshot["worldInfoActivation"]["entries"], json!([]));
    assert_eq!(snapshot["macroContext"]["names"]["char"], "Char");
    assert_eq!(snapshot["variables"]["local"]["score"], json!(42));
    assert_eq!(snapshot["variables"]["global"]["theme"], json!("dark"));
    assert_eq!(
        snapshot["currentModelConnection"]["settings"]["custom_url"],
        "https://opencode.example.test/v1"
    );
    assert_eq!(
        snapshot["currentModelConnection"]["settings"]["secret_id"],
        "opencode-secret"
    );
}

#[test]
fn frozen_run_input_snapshot_variables_is_optional() {
    let snapshot = normalize_frozen_run_input_snapshot(
        &json!({
            "schemaVersion": 1,
            "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
            "generationType": "normal",
            "promptInputs": { "type": "normal", "messages": [] },
            "worldInfoActivation": { "entries": [] },
            "macroContext": {},
        }),
        "normal",
    )
    .unwrap();

    assert!(snapshot.get("variables").is_none());
}

#[test]
fn builds_current_model_connection_snapshot_with_backend_owned_fields() {
    let snapshot = build_current_model_connection_snapshot(
        &json!({
            "chat_completion_source": "aws_bedrock",
            "aws_bedrock_model": "amazon.titan-text-premier-v1:0",
            "aws_bedrock_region": "eu-central-1",
            "aws_bedrock_use_custom_template": true,
            "aws_bedrock_custom_template": "{\"inputText\":{{messages}}}",
            "aws_bedrock_custom_response_path": "results.0.outputText",
            "aws_bedrock_custom_stream_path": "delta.text",
            "additional_parameters_by_source": {
                "aws_bedrock": {
                    "include_body": "",
                    "exclude_body": "",
                    "include_headers": "X-Trace: frozen"
                }
            },
            "custom_claude_prompt_caching": true,
            "custom_models_by_source": { "aws_bedrock": ["catalog-only"] },
            "openrouter_group_models": true,
            "openrouter_sort_models": "context",
            "show_external_models": true,
            "additional_parameters_migration_version": 1,
            "bypass_status_check": true
        }),
        "amazon.titan-text-premier-v1:0",
        Some("bedrock-secret"),
    )
    .unwrap();
    let settings = snapshot["settings"].as_object().unwrap();

    assert_eq!(settings["chat_completion_source"], "aws_bedrock");
    assert_eq!(settings["model"], "amazon.titan-text-premier-v1:0");
    assert_eq!(
        settings["aws_bedrock_model"],
        "amazon.titan-text-premier-v1:0"
    );
    assert_eq!(settings["aws_bedrock_region"], "eu-central-1");
    assert_eq!(settings["aws_bedrock_use_custom_template"], true);
    assert_eq!(
        settings["aws_bedrock_custom_response_path"],
        "results.0.outputText"
    );
    assert_eq!(
        settings["additional_parameters_by_source"]["aws_bedrock"]["include_headers"],
        "X-Trace: frozen"
    );
    assert_eq!(settings["custom_claude_prompt_caching"], true);
    assert_eq!(settings["secret_id"], "bedrock-secret");
    assert!(settings.get("custom_models_by_source").is_none());
    assert!(settings.get("openrouter_group_models").is_none());
    assert!(settings.get("openrouter_sort_models").is_none());
    assert!(settings.get("show_external_models").is_none());
    assert!(settings.get("bypass_status_check").is_none());
    assert!(
        settings
            .get("additional_parameters_migration_version")
            .is_none()
    );
}

#[test]
fn builds_current_model_connection_snapshot_with_openrouter_routing_fields() {
    let snapshot = build_current_model_connection_snapshot(
        &json!({
            "chat_completion_source": "openrouter",
            "openrouter_model": "anthropic/claude-sonnet-4",
            "openrouter_use_fallback": true,
            "openrouter_providers": ["anthropic", "openai"],
            "openrouter_quantizations": ["bf16"],
            "openrouter_allow_fallbacks": false,
            "openrouter_middleout": "off",
            "openrouter_group_models": true,
            "openrouter_sort_models": "context",
            "custom_models_by_source": { "openrouter": ["catalog-only"] }
        }),
        "anthropic/claude-sonnet-4",
        Some("openrouter-secret"),
    )
    .unwrap();
    let settings = snapshot["settings"].as_object().unwrap();

    assert_eq!(settings["chat_completion_source"], "openrouter");
    assert_eq!(settings["model"], "anthropic/claude-sonnet-4");
    assert_eq!(settings["openrouter_model"], "anthropic/claude-sonnet-4");
    assert_eq!(settings["openrouter_use_fallback"], true);
    assert_eq!(
        settings["openrouter_providers"],
        json!(["anthropic", "openai"])
    );
    assert_eq!(settings["openrouter_quantizations"], json!(["bf16"]));
    assert_eq!(settings["openrouter_allow_fallbacks"], false);
    assert_eq!(settings["openrouter_middleout"], "off");
    assert_eq!(settings["secret_id"], "openrouter-secret");
    assert!(settings.get("openrouter_group_models").is_none());
    assert!(settings.get("openrouter_sort_models").is_none());
    assert!(settings.get("custom_models_by_source").is_none());
}

#[test]
fn current_model_connection_snapshot_rejects_unmapped_source() {
    let error = build_current_model_connection_snapshot(
        &json!({
            "chat_completion_source": "unsupported",
            "custom_url": "https://example.test/v1"
        }),
        "local-model",
        None,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("prompt_assembly.model_source_unmapped")
    );
}

#[test]
fn rejects_frozen_snapshot_generation_type_mismatch() {
    let error = normalize_frozen_run_input_snapshot(
        &json!({
            "schemaVersion": 1,
            "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
            "generationType": "normal",
            "promptInputs": {},
            "worldInfoActivation": { "entries": [] },
            "macroContext": {},
        }),
        "regenerate",
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("prompt_assembly.generation_type_mismatch")
    );
}

#[test]
fn overlays_connection_ref_model_without_preset_source() {
    let mut settings = json!({
        "name": "Prompt Only",
        "temp_openai": 0.7,
        "custom_url": "https://stale.example.test",
        "openrouter_model": "anthropic/claude"
    });
    let binding = model_binding("deepseek", "deepseek-v4-flash", None);

    apply_model_binding_to_prompt_settings(&mut settings, &binding).unwrap();

    assert_eq!(settings["chat_completion_source"], "deepseek");
    assert_eq!(settings["deepseek_model"], "deepseek-v4-flash");
    assert_eq!(settings["temp_openai"], 0.7);
    assert!(settings.get("custom_url").is_none());
    assert!(settings.get("openrouter_model").is_none());
}

#[test]
fn connection_ref_model_overrides_conflicting_preset_source() {
    let mut settings = json!({
        "chat_completion_source": "openrouter",
        "openrouter_model": "anthropic/claude",
        "deepseek_model": "deepseek-chat"
    });
    let binding = model_binding("deepseek", "deepseek-v4-flash", None);

    apply_model_binding_to_prompt_settings(&mut settings, &binding).unwrap();

    assert_eq!(settings["chat_completion_source"], "deepseek");
    assert_eq!(settings["deepseek_model"], "deepseek-v4-flash");
    assert!(settings.get("openrouter_model").is_none());
}

#[test]
fn custom_connection_ref_sets_custom_format_and_model() {
    let mut settings = json!({
        "chat_completion_source": "deepseek",
        "deepseek_model": "deepseek-v4-flash"
    });
    let binding = model_binding("custom", "local-model", Some("gemini_interactions"));

    apply_model_binding_to_prompt_settings(&mut settings, &binding).unwrap();

    assert_eq!(settings["chat_completion_source"], "custom");
    assert_eq!(settings["custom_model"], "local-model");
    assert_eq!(settings["custom_api_format"], "gemini_interactions");
    assert!(settings.get("deepseek_model").is_none());
}

#[test]
fn current_prompt_snapshot_overlays_connection_settings_from_frozen_snapshot() {
    let mut settings = json!({
        "name": "Prompt Only",
        "temp_openai": 0.7,
        "chat_completion_source": "custom",
        "custom_model": "old-opencode-model",
        "custom_url": "https://opencode.example.test/v1",
        "secret_id": "old-secret",
        "openrouter_providers": ["stale-provider"],
        "openrouter_quantizations": ["stale-quantization"],
        "openrouter_allow_fallbacks": false,
        "openrouter_middleout": "off",
        "additional_parameters_by_source": {
            "custom": {
                "include_body": "",
                "exclude_body": "",
                "include_headers": "X-Preset: stale"
            }
        },
        "custom_claude_prompt_caching": false
    });
    let frozen_run_input_snapshot = normalize_frozen_run_input_snapshot(
        &json!({
            "schemaVersion": 1,
            "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
            "generationType": "normal",
            "promptInputs": {},
            "worldInfoActivation": {},
            "macroContext": {},
            "currentModelConnection": {
                "schemaVersion": 1,
                "kind": CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
                "settings": {
                    "chat_completion_source": "custom",
                    "model": "deepseek-chat-through-custom",
                    "custom_model": "deepseek-chat-through-custom",
                    "custom_url": "https://api.deepseek.example/v1",
                    "custom_api_format": "openai_compat",
                    "secret_id": "deepseek-secret",
                    "additional_parameters_by_source": {
                        "custom": {
                            "include_body": "",
                            "exclude_body": "",
                            "include_headers": "X-Run: current"
                        }
                    },
                    "custom_claude_prompt_caching": true
                }
            }
        }),
        "normal",
    )
    .unwrap();

    apply_current_model_connection_to_prompt_settings(&mut settings, &frozen_run_input_snapshot)
        .unwrap();

    assert_eq!(settings["chat_completion_source"], "custom");
    assert_eq!(settings["custom_model"], "deepseek-chat-through-custom");
    assert_eq!(settings["custom_url"], "https://api.deepseek.example/v1");
    assert_eq!(settings["custom_api_format"], "openai_compat");
    assert_eq!(settings["secret_id"], "deepseek-secret");
    assert_eq!(
        settings["additional_parameters_by_source"]["custom"]["include_headers"],
        "X-Run: current"
    );
    assert_eq!(settings["custom_claude_prompt_caching"], true);
    assert!(settings.get("openrouter_providers").is_none());
    assert!(settings.get("openrouter_quantizations").is_none());
    assert!(settings.get("openrouter_allow_fallbacks").is_none());
    assert!(settings.get("openrouter_middleout").is_none());
    assert_eq!(settings["temp_openai"], 0.7);
}

#[test]
fn current_prompt_snapshot_removes_stale_secret_when_current_connection_is_keyless() {
    let mut settings = json!({
        "chat_completion_source": "custom",
        "custom_model": "old-model",
        "custom_url": "https://old.example.test/v1",
        "secret_id": "old-secret"
    });
    let frozen_run_input_snapshot = normalize_frozen_run_input_snapshot(
        &json!({
            "schemaVersion": 1,
            "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
            "generationType": "normal",
            "promptInputs": {},
            "worldInfoActivation": {},
            "macroContext": {},
            "currentModelConnection": {
                "schemaVersion": 1,
                "kind": CURRENT_MODEL_CONNECTION_SNAPSHOT_KIND,
                "settings": {
                    "chat_completion_source": "custom",
                    "model": "local-model",
                    "custom_model": "local-model",
                    "custom_url": "http://127.0.0.1:8000/v1",
                    "custom_api_format": "openai_compat"
                }
            }
        }),
        "normal",
    )
    .unwrap();

    apply_current_model_connection_to_prompt_settings(&mut settings, &frozen_run_input_snapshot)
        .unwrap();

    assert_eq!(settings["custom_model"], "local-model");
    assert_eq!(settings["custom_url"], "http://127.0.0.1:8000/v1");
    assert!(settings.get("secret_id").is_none());
}

#[test]
fn current_prompt_snapshot_requires_frozen_current_model_connection() {
    let mut settings = json!({
        "chat_completion_source": "custom",
        "custom_model": "old-model"
    });
    let frozen_run_input_snapshot = normalize_frozen_run_input_snapshot(
        &json!({
            "schemaVersion": 1,
            "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
            "generationType": "normal",
            "promptInputs": {},
            "worldInfoActivation": {},
            "macroContext": {}
        }),
        "normal",
    )
    .unwrap();

    let error = apply_current_model_connection_to_prompt_settings(
        &mut settings,
        &frozen_run_input_snapshot,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("prompt_assembly.current_model_connection_required")
    );
}
