use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

use crate::app::host::request_frontend_shutdown;
use crate::presentation::main_window_presenter::present_main_window;

const TRAY_ID: &str = "tauritavern-tray";
const MENU_SHOW_ID: &str = "tauritavern-tray:show";
const MENU_EXIT_ID: &str = "tauritavern-tray:exit";

pub struct WindowsTrayState {
    close_to_tray_on_close: AtomicBool,
}

impl WindowsTrayState {
    pub fn new(close_to_tray_on_close: bool) -> Self {
        Self {
            close_to_tray_on_close: AtomicBool::new(close_to_tray_on_close),
        }
    }

    pub fn close_to_tray_on_close(&self) -> bool {
        self.close_to_tray_on_close.load(Ordering::Relaxed)
    }

    pub fn set_close_to_tray_on_close(&self, enabled: bool) {
        self.close_to_tray_on_close
            .store(enabled, Ordering::Relaxed);
    }
}

pub fn install_windows_tray(
    app_handle: &AppHandle,
    main_window: &tauri::webview::WebviewWindow,
    state: Arc<WindowsTrayState>,
) -> tauri::Result<()> {
    let main_window = main_window.clone();

    let show_item = MenuItemBuilder::with_id(MENU_SHOW_ID, "Show").build(app_handle)?;
    let exit_item = MenuItemBuilder::with_id(MENU_EXIT_ID, "Exit").build(app_handle)?;
    let separator = PredefinedMenuItem::separator(app_handle)?;

    let menu = Menu::with_items(app_handle, &[&show_item, &separator, &exit_item])?;

    let icon = app_handle
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("Default window icon is missing".into()))?;

    let main_window_for_menu = main_window.clone();

    let main_window_for_tray = main_window.clone();

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("TauriTavern")
        .menu(&menu)
        .on_menu_event(move |_app, event| match event.id().as_ref() {
            MENU_SHOW_ID => {
                if let Err(error) = present_main_window(&main_window_for_menu) {
                    tracing::warn!("Failed to show main window from tray menu: {}", error);
                }
            }
            MENU_EXIT_ID => {
                if let Err(error) = request_frontend_shutdown(&main_window_for_menu) {
                    tracing::error!("Failed to request graceful shutdown: {error}");
                }
            }
            _ => {}
        })
        .on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
                && let Err(error) = present_main_window(&main_window_for_tray)
            {
                tracing::warn!("Failed to show main window from tray icon: {}", error);
            }
        })
        .build(app_handle)?;

    let state_for_close = state.clone();
    let main_window_for_close = main_window.clone();
    main_window.on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };

        if !state_for_close.close_to_tray_on_close() {
            match request_frontend_shutdown(&main_window_for_close) {
                Ok(()) => api.prevent_close(),
                Err(error) => tracing::error!("Failed to request graceful shutdown: {error}"),
            }
            return;
        }

        api.prevent_close();
        main_window_for_close
            .hide()
            .expect("Failed to hide main window on close");
    });

    // Keep the tray state alive for the lifetime of the app.
    app_handle.manage(state);

    Ok(())
}
