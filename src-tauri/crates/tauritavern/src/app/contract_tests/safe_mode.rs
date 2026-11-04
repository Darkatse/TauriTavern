use super::*;
use tt_adapter_extension::FileExtensionRepository;
use tt_adapter_media::{FileAvatarRepository, FilesystemHostResourceStore};
use tt_adapter_storage_core::DataDirectory;
use tt_adapter_sync::SyncAutomationStore;
use tt_application::services::safe_mode_service::SafeModeService;
use tt_contracts::settings::PERSONA_STATE_FILE;
use tt_domain::models::settings::{ChatBackupSettings, TauriTavernSettings};
use tt_ports::repositories::settings_repository::SettingsRepository;

const FACTORY_USER_CSS: &[u8] = b"/* Put custom styles here. */\n";

struct Fixture {
    root: PathBuf,
    data: DataDirectory,
    settings: Arc<FileSettingsRepository>,
    sync_rules: Arc<SyncAutomationStore>,
    service: SafeModeService,
}

fn fixture(label: &str) -> Fixture {
    let root = temp_root(label);
    let data = DataDirectory::new(root.clone());
    let settings = Arc::new(FileSettingsRepository::new(
        data.settings().to_path_buf(),
        UserSettings {
            data: json!({
                "firstRun": true,
                "extension_settings": { "disabledExtensions": ["memory"] },
                "power_user": { "theme": "Default" },
            }),
        },
        TauriTavernSettings::default(),
        Arc::default(),
        4 * 1024 * 1024,
    ));
    let sync_rules = Arc::new(SyncAutomationStore::new(data.default_user().to_path_buf()));
    let service = SafeModeService::new(
        settings.clone(),
        Arc::new(FileAvatarRepository::new(
            data.default_user().join("User Avatars"),
        )),
        Arc::new(FileExtensionRepository::new(
            data.default_user().join("extensions"),
            data.global_extensions().to_path_buf(),
            data.extension_sources().to_path_buf(),
            Arc::new(HttpClientPool::new("tauritavern-contract-test")),
        )),
        Arc::new(FilesystemHostResourceStore::from_data_root(&root)),
        sync_rules.clone(),
        FACTORY_USER_CSS.to_vec(),
    );
    Fixture {
        root,
        data,
        settings,
        sync_rules,
        service,
    }
}

fn has_safe_mode_copy(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(&format!("{name}.safe-mode-"))
    })
}

#[tokio::test]
async fn safe_mode_resets_the_customization_layer_and_keeps_content() {
    let Fixture {
        root,
        data,
        settings,
        sync_rules,
        service,
    } = fixture("safe-mode");
    settings
        .save_user_settings(UserSettings {
            data: json!({
                "firstRun": false,
                "username": "Alice",
                "user_avatar": "alice.png",
                "active_character": "Heavy.png",
                "extension_settings": { "disabledExtensions": [], "Foo": { "blob": "x" } },
                "power_user": { "theme": "Broken", "custom_css": "body { display: none }" },
            }),
        })
        .await
        .unwrap();
    let persona_state = std::fs::read(data.settings().join(PERSONA_STATE_FILE)).unwrap();
    for extension in [
        data.default_user().join("extensions").join("Foo"),
        data.global_extensions().join("Bar"),
    ] {
        std::fs::create_dir_all(&extension).unwrap();
        std::fs::write(extension.join("manifest.json"), "{}").unwrap();
    }
    let chat_backups = ChatBackupSettings {
        max_files_per_prefix: -1,
        ..ChatBackupSettings::default()
    };
    let mut native = TauriTavernSettings {
        chat_backups,
        ..TauriTavernSettings::default()
    };
    native.request_proxy.enabled = true;
    native.dynamic_theme.enabled = true;
    settings.save_tauritavern_settings(&native).await.unwrap();
    let user_css = root.join("_css").join("user.css");
    std::fs::create_dir_all(user_css.parent().unwrap()).unwrap();
    std::fs::write(&user_css, "body { display: none }").unwrap();
    let mut rule = sync_rules.load_or_create_rule().await.unwrap().rule;
    rule.enabled = true;
    sync_rules.save_rule(&rule).await.unwrap();

    let report = service.rescue().await.unwrap();

    assert_eq!(report.disabled_extensions, 2);
    assert!(report.auto_upload_paused);
    assert!(report.user_css_reset);
    let snapshot = settings.load_snapshot(&report.snapshot).await.unwrap().data;
    assert_eq!(
        snapshot["power_user"]["custom_css"],
        "body { display: none }"
    );

    let reset = settings.load_user_settings().await.unwrap().settings.data;
    assert_eq!(reset["firstRun"], false);
    assert_eq!(
        reset["extension_settings"]["disabledExtensions"],
        json!(["memory", "third-party/Foo", "third-party/Bar"])
    );
    assert_eq!(reset["power_user"]["theme"], "Default");
    assert!(reset["power_user"].get("custom_css").is_none());
    assert!(reset.get("active_character").is_none());
    assert_eq!(reset["username"], "Alice");
    assert_eq!(
        std::fs::read(data.settings().join(PERSONA_STATE_FILE)).unwrap(),
        persona_state
    );

    let native = settings.load_tauritavern_settings().await.unwrap();
    assert_eq!(native.chat_backups, chat_backups);
    assert!(!native.request_proxy.enabled);
    assert!(!native.dynamic_theme.enabled);
    assert!(has_safe_mode_copy(
        data.settings(),
        "tauritavern-settings.json"
    ));
    assert_eq!(std::fs::read(&user_css).unwrap(), FACTORY_USER_CSS);
    assert!(has_safe_mode_copy(user_css.parent().unwrap(), "user.css"));
    assert!(!sync_rules.load_or_create_rule().await.unwrap().rule.enabled);

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn safe_mode_replaces_chat_backup_limits_that_would_fail_startup_with_none() {
    let Fixture {
        root,
        settings,
        service,
        ..
    } = fixture("safe-mode-invalid-limits");
    let native = TauriTavernSettings {
        chat_backups: ChatBackupSettings {
            max_total_bytes: -2,
            ..ChatBackupSettings::default()
        },
        ..TauriTavernSettings::default()
    };
    settings.save_tauritavern_settings(&native).await.unwrap();

    service.rescue().await.unwrap();

    let limits = settings
        .load_tauritavern_settings()
        .await
        .unwrap()
        .chat_backups;
    assert_eq!(
        [
            limits.max_files_per_prefix,
            limits.max_total_files,
            limits.max_total_bytes,
        ],
        [-1; 3]
    );
    std::fs::remove_dir_all(root).unwrap();
}
