// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    apply_linux_webkit_workarounds();
    tauritavern_lib::run()
}

#[cfg(target_os = "linux")]
fn apply_linux_webkit_workarounds() {
    const WEBKIT_DMABUF_ENV: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    const OPT_OUT_ENV: &str = "TAURITAVERN_DISABLE_WEBKIT_DMABUF_WORKAROUND";

    if std::env::var_os(WEBKIT_DMABUF_ENV).is_some() || std::env::var_os(OPT_OUT_ENV).is_some() {
        return;
    }

    // WebKitGTK's DMA-BUF renderer can crash in some Linux GPU/driver stacks.
    // See https://github.com/tauri-apps/tauri/issues/9394 for details.
    // Set this before Tauri/WebKit starts so spawned WebKitWebProcess instances inherit it.
    unsafe {
        std::env::set_var(WEBKIT_DMABUF_ENV, "1");
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_webkit_workarounds() {}
