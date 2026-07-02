use tauri::webview::WebviewWindow;
use tauri::{AppHandle, Manager};

const MAIN_WINDOW_LABEL: &str = "main";

pub fn present_main_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.show()?;
    window.unminimize()?;
    window.set_focus()
}

pub fn present_main_window_from_app(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or(tauri::Error::WindowNotFound)?;

    present_main_window(&window)
}
