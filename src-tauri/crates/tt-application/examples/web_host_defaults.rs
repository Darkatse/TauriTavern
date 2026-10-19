use tt_application::dto::settings_dto::TauriTavernSettingsDto;
use tt_domain::models::settings::TauriTavernSettings;

fn main() -> Result<(), serde_json::Error> {
    let defaults = TauriTavernSettingsDto::from(TauriTavernSettings::default());
    println!("{}", serde_json::to_string_pretty(&defaults)?);
    Ok(())
}
