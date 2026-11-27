use super::*;
use tt_adapter_media::FileAvatarRepository;
use tt_application::services::generation_runtime::GenerationRuntime;
use tt_application::services::settings_service::SettingsService;

fn settings_service(root: &Path) -> SettingsService {
    SettingsService::new(
        Arc::new(FileAvatarRepository::new(root.join("User Avatars"))),
        Arc::new(FileSettingsRepository::new(
            root.to_path_buf(),
            JsonDocument::default(),
            Default::default(),
            Arc::default(),
            4 * 1024 * 1024,
        )),
        Arc::new(HttpClientPool::new("TauriTavern contract tests")),
        Arc::new(FileChatRepository::new(
            root.join("characters"),
            root.join("chats"),
            root.join("group chats"),
            root.join("backups"),
        )),
        Arc::new(GenerationRuntime::new(None, true)),
    )
}

async fn commit(service: &SettingsService, settings: &str) {
    let commit = service.begin_commit(None).await.unwrap();
    service
        .append_commit(&commit.session_id, 0, settings.as_bytes())
        .await
        .unwrap();
    service
        .finish_commit(&commit.session_id, settings.len() as u64)
        .await
        .unwrap();
}

async fn read_settings(service: &SettingsService) -> String {
    let response = service.get_sillytavern_settings().await.unwrap();
    let response: Value = serde_json::from_slice(&response.bytes).unwrap();
    response["settings"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn committed_settings_read_back_as_the_text_that_was_committed() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let service = settings_service(temp.path());
    // Rust strings cannot hold the lone surrogate that JavaScript writes as this escape.
    let extension_settings = r#"{"zeta":1,"alpha":{"b":"\ud83d","a":2},"mid":[3]}"#;
    commit(
        &service,
        &format!(r#"{{"firstRun":false,"extension_settings":{extension_settings}}}"#),
    )
    .await;

    let settings = read_settings(&service).await;
    assert!(settings.contains(extension_settings), "{settings}");
}

#[tokio::test]
async fn restoring_a_snapshot_that_cannot_be_split_changes_nothing() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let service = settings_service(temp.path());
    commit(&service, r#"{"marker":"old"}"#).await;
    // Splitting opens accountStorage, whose key Rust cannot decode.
    let snapshot = "settings_default-user_20260101-000000.json";
    let backups = temp.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::write(
        backups.join(snapshot),
        br#"{"power_user":{"personas":{"review.png":"Alice"}},"accountStorage":{"\ud800":1}}"#,
    )
    .unwrap();

    assert!(service.restore_snapshot(snapshot).await.is_err());

    assert!(!temp.path().join("User Avatars/review.png").exists());
    let settings = read_settings(&service).await;
    assert!(settings.contains(r#""marker":"old""#), "{settings}");
}
