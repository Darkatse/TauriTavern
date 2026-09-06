use super::*;
use tt_domain::models::llm_connection::LlmConnectionDefinition;
use tt_domain::models::settings::UserSettings;
use tt_ports::repositories::settings_repository::SettingsRepository;

#[tokio::test]
async fn agent_model_binding_uses_the_named_proxy_without_copying_credentials() {
    let root = temp_root("agent-proxy-binding");
    let settings = Arc::new(FileSettingsRepository::new(root.join("default-user")));
    let service = LlmConnectionService::new(
        Arc::new(FileLlmConnectionRepository::new(
            root.join("llm-connections"),
        )),
        settings.clone(),
    );
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
        settings.save_user_settings(&UserSettings { data: json!({
            "proxies": [
                {"name": "Unrelated proxy", "url": "https://other.example", "password": "other"},
                {"name": "Team proxy", "url": url, "password": password}
            ],
            "selected_proxy": {"name": "Unrelated proxy"}
        }) }).await.unwrap();
        let mut payload = json!({
            "reverse_proxy": "https://stale.example", "proxy_password": "stale", "messages": []
        })
        .as_object()
        .unwrap()
        .clone();
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
        .save_user_settings(&UserSettings {
            data: json!({"proxies": []}),
        })
        .await
        .unwrap();
    let error = service
        .apply_connection_to_payload(
            "proxy-target",
            "[v]gemini-custom",
            &mut serde_json::Map::new(),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("llm_connection.proxy_preset_missing")
    );
    let _ = fs::remove_dir_all(root).await;
}
