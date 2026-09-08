use tauri::AppHandle;
use tt_domain::errors::DomainError;

pub(crate) async fn set_lan_discovery_enabled(
    app_handle: &AppHandle,
    enabled: bool,
) -> Result<(), DomainError> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;

        let platform = app_handle
            .try_state::<AndroidLanDiscovery<tauri::Wry>>()
            .ok_or_else(|| {
                DomainError::InternalError(
                    "Android LAN discovery plugin is unavailable".to_string(),
                )
            })?;
        let handle = platform.handle.as_ref().map_err(|error| {
            DomainError::InternalError(format!(
                "Android LAN discovery plugin is unavailable: {error}"
            ))
        })?;
        handle
            .run_mobile_plugin_async::<()>("setEnabled", serde_json::json!({ "enabled": enabled }))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to change Android LAN discovery multicast access: {error}"
                ))
            })
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = (app_handle, enabled);
        Ok(())
    }
}

#[cfg(target_os = "android")]
struct AndroidLanDiscovery<R: tauri::Runtime> {
    handle: Result<tauri::plugin::PluginHandle<R>, String>,
}

#[cfg(target_os = "android")]
pub(crate) fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri::Manager;

    tauri::plugin::Builder::new("lan-discovery")
        .setup(|app, api| {
            let handle = api
                .register_android_plugin("com.tauritavern.client", "LanDiscoveryPlugin")
                .map_err(|error| error.to_string());
            // Keep a discovery failure local to discovery; existing HTTPS Sync can still run.
            app.manage(AndroidLanDiscovery { handle });
            Ok(())
        })
        .build()
}
