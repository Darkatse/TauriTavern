use super::*;
use tt_domain::models::llm_connection::LlmConnectionDefinition;
use tt_ports::repositories::settings_repository::SettingsRepository;

fn connection_service(
    root: &std::path::Path,
) -> (Arc<FileSettingsRepository>, LlmConnectionService) {
    let settings = Arc::new(FileSettingsRepository::new(
        root.join("default-user"),
        JsonDocument::default(),
        Default::default(),
        Arc::default(),
        4 * 1024 * 1024,
    ));
    let service = LlmConnectionService::new(
        Arc::new(FileLlmConnectionRepository::new(
            root.join("llm-connections"),
        )),
        settings.clone(),
    );
    (settings, service)
}

fn payload_with_stale_proxy() -> serde_json::Map<String, serde_json::Value> {
    json!({
        "reverse_proxy": "https://stale.example", "proxy_password": "stale", "messages": []
    })
    .as_object()
    .unwrap()
    .clone()
}

#[tokio::test]
async fn agent_model_binding_follows_the_named_proxy_preset() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let (settings, service) = connection_service(temp.path());
    let connection: LlmConnectionDefinition = serde_json::from_value(json!({
        "schemaVersion": 1,
        "kind": "tauritavern.llmConnection",
        "id": "proxy-target",
        "displayName": "Proxy model",
        "provider": {"chatCompletionSource": "makersuite"},
        "auth": {},
        "routing": {"reverseProxy": {"preset": "Team proxy"}}
    }))
    .unwrap();
    service.save_connection(connection).await.unwrap();

    for (url, password) in [
        ("https://proxy.example/v1beta", "first-password"),
        ("https://new-proxy.example/v1beta", "updated-password"),
    ] {
        settings
            .save_user_settings(user_settings(json!({
                "proxies": [
                    {"name": "Unrelated proxy", "url": "https://other.example", "password": "other"},
                    {"name": "Team proxy", "url": url, "password": password}
                ],
                "selected_proxy": {"name": "Unrelated proxy"}
            })))
            .await
            .unwrap();
        let mut payload = payload_with_stale_proxy();
        service
            .apply_connection_to_payload("proxy-target", "[v]gemini-custom", &mut payload)
            .await
            .unwrap();
        assert_eq!(payload["reverse_proxy"], url);
        assert_eq!(payload["proxy_password"], password);
        assert_eq!(payload["model"], "[v]gemini-custom");
        assert!(!payload.contains_key("secret_id"));
    }

    settings
        .save_user_settings(user_settings(json!({
            "proxies": [{"name": "Team proxy", "url": "", "password": "unused"}]
        })))
        .await
        .unwrap();
    let mut payload = payload_with_stale_proxy();
    service
        .apply_connection_to_payload("proxy-target", "[v]gemini-custom", &mut payload)
        .await
        .unwrap();
    assert!(!payload.contains_key("reverse_proxy"));
    assert!(!payload.contains_key("proxy_password"));

    settings
        .save_user_settings(user_settings(json!({"proxies": []})))
        .await
        .unwrap();
    for error in [
        service
            .resolve_model_binding("proxy-target", "[v]gemini-custom")
            .await
            .unwrap_err(),
        service
            .apply_connection_to_payload(
                "proxy-target",
                "[v]gemini-custom",
                &mut serde_json::Map::new(),
            )
            .await
            .unwrap_err(),
    ] {
        assert!(
            error
                .to_string()
                .contains("llm_connection.proxy_preset_missing")
        );
    }
}

#[tokio::test]
async fn agent_model_binding_custom_connections_need_no_key_and_take_no_proxy() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let (_settings, service) = connection_service(temp.path());
    let mut custom: LlmConnectionDefinition = serde_json::from_value(json!({
        "schemaVersion": 1,
        "kind": "tauritavern.llmConnection",
        "id": "keyless-custom",
        "displayName": "Local server",
        "provider": {"chatCompletionSource": "custom"},
        "endpoint": {"baseUrl": "http://127.0.0.1:5001/v1"},
        "auth": {},
        "routing": {"reverseProxy": {"preset": "NONE"}}
    }))
    .unwrap();
    let error = service.save_connection(custom.clone()).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("llm_connection.reverse_proxy_unsupported")
    );

    custom.routing.reverse_proxy = None;
    service.save_connection(custom).await.unwrap();
    let mut payload = serde_json::Map::new();
    service
        .apply_connection_to_payload("keyless-custom", "local-model", &mut payload)
        .await
        .unwrap();
    assert_eq!(payload["custom_url"], "http://127.0.0.1:5001/v1");
    assert!(!payload.contains_key("secret_id"));
}
