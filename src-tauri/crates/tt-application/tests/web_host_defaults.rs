use tt_application::dto::settings_dto::TauriTavernSettingsDto;
use tt_domain::models::settings::TauriTavernSettings;

#[test]
fn web_host_starts_with_the_native_settings_dto() {
    let published: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../src/tauri/web-host/default-settings.json"
    ))
    .unwrap();
    let native = TauriTavernSettingsDto::from(TauriTavernSettings::default());
    assert_eq!(
        published,
        serde_json::to_value(native).unwrap(),
        "TauriTavern settings defaults changed; regenerate the Web Host copy:\n  \
         cargo run --quiet --manifest-path src-tauri/Cargo.toml -p tt-application \
         --example web_host_defaults > src/tauri/web-host/default-settings.json",
    );
}
