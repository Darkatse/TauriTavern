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
fn attaches_selected_frozen_input_and_preserves_optional_input() {
    let prompt = json!({ "messages": [] });
    assert_eq!(
        attach_frozen_run_input_snapshot(prompt.clone(), None).unwrap(),
        prompt
    );

    let frozen = json!({
        "schemaVersion": 1,
        "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
        "generationType": "normal",
        "promptInputs": {},
        "worldInfoActivation": {},
        "macroContext": { "names": { "char": "Char" } }
    });
    let embedded = json!({ "messages": [], "frozenRunInputSnapshot": frozen });
    assert_eq!(
        attach_frozen_run_input_snapshot(embedded.clone(), None).unwrap(),
        embedded
    );
    // The explicit input replaces the embedded input; only the selected one is validated.
    assert_eq!(
        attach_frozen_run_input_snapshot(
            json!({ "messages": [], "frozenRunInputSnapshot": 7 }),
            Some(frozen),
        )
        .unwrap(),
        embedded
    );
}

#[test]
fn rejects_malformed_embedded_frozen_input() {
    for (frozen, error_code) in [
        (json!(7), "agent.frozen_run_input_snapshot_required"),
        (
            json!({
                "schemaVersion": 1,
                "kind": FROZEN_RUN_INPUT_SNAPSHOT_KIND,
                "generationType": "normal",
                "promptInputs": {},
                "worldInfoActivation": {},
                "macroContext": 7
            }),
            "agent.frozen_run_input_macro_context_invalid",
        ),
    ] {
        let error =
            attach_frozen_run_input_snapshot(json!({ "frozenRunInputSnapshot": frozen }), None)
                .unwrap_err();
        assert!(matches!(error, ApplicationError::ValidationError(_)));
        assert!(error.to_string().contains(error_code), "{error}");
    }
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
