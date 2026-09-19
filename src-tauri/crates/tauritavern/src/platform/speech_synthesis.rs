pub(crate) fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("speech-synthesis")
        .js_init_script_on_all_frames(include_str!(
            "../../../../../src/tauri/main/compat/android-speech-synthesis.js"
        ))
        .setup(|_app, api| {
            if let Err(error) =
                api.register_android_plugin("com.tauritavern.client", "SpeechSynthesisPlugin")
            {
                tracing::warn!("Android system TTS is unavailable: {error}");
            }
            Ok(())
        })
        .build()
}
