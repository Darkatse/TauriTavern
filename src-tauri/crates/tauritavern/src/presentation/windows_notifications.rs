use std::path::MAIN_SEPARATOR as SEP;

use tauri::AppHandle;
use tauri_winrt_notification::{Duration, Toast};

use crate::presentation::main_window_presenter::present_main_window_from_app;

pub fn show_system_notification(
    app: &AppHandle,
    title: &str,
    body: &str,
) -> tauri_winrt_notification::Result<()> {
    let app_id = notification_app_id(app)?;
    let app_for_activation = app.clone();

    Toast::new(&app_id)
        .title(title)
        .text1("")
        .text2(body)
        .sound(None)
        .duration(Duration::Short)
        .on_activated(move |_| {
            present_main_window_after_notification_click(&app_for_activation);
            Ok(())
        })
        .show()
}

fn present_main_window_after_notification_click(app: &AppHandle) {
    let app_for_thread = app.clone();

    if let Err(error) = app.run_on_main_thread(move || {
        if let Err(error) = present_main_window_from_app(&app_for_thread) {
            tracing::warn!(
                "Failed to present main window from notification activation: {}",
                error
            );
        }
    }) {
        tracing::warn!(
            "Failed to schedule main window presentation from notification activation: {}",
            error
        );
    }
}

fn notification_app_id(app: &AppHandle) -> tauri_winrt_notification::Result<String> {
    let exe = tauri::utils::platform::current_exe()?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("Failed to resolve executable directory"))?;
    let exe_dir = exe_dir.display().to_string();

    if should_use_unregistered_app_id(&exe_dir) {
        Ok(Toast::POWERSHELL_APP_ID.to_string())
    } else {
        Ok(app.config().identifier.clone())
    }
}

fn should_use_unregistered_app_id(exe_dir: &str) -> bool {
    cfg!(feature = "portable")
        || exe_dir.ends_with(&format!("{SEP}target{SEP}debug"))
        || exe_dir.ends_with(&format!("{SEP}target{SEP}release"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_unregistered_app_id_for_cargo_target_build_dirs() {
        assert!(should_use_unregistered_app_id(&format!(
            "C:{SEP}repo{SEP}target{SEP}debug"
        )));
        assert!(should_use_unregistered_app_id(&format!(
            "C:{SEP}repo{SEP}target{SEP}release"
        )));
    }

    #[cfg(not(feature = "portable"))]
    #[test]
    fn uses_configured_app_id_for_installed_builds() {
        assert!(!should_use_unregistered_app_id(&format!(
            "C:{SEP}Program Files{SEP}TauriTavern"
        )));
    }

    #[cfg(feature = "portable")]
    #[test]
    fn uses_unregistered_app_id_for_portable_builds() {
        assert!(should_use_unregistered_app_id(&format!(
            "C:{SEP}Program Files{SEP}TauriTavern"
        )));
    }
}
