use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

const TRAY_ID: &str = "tauritavern-tray";
const MENU_SHOW_ID: &str = "tauritavern-tray:show";
const MENU_EXIT_ID: &str = "tauritavern-tray:exit";

pub struct WindowsTrayState {
    close_to_tray_on_close: AtomicBool,
    quitting: AtomicBool,
}

impl WindowsTrayState {
    pub fn new(close_to_tray_on_close: bool) -> Self {
        Self {
            close_to_tray_on_close: AtomicBool::new(close_to_tray_on_close),
            quitting: AtomicBool::new(false),
        }
    }

    pub fn close_to_tray_on_close(&self) -> bool {
        self.close_to_tray_on_close.load(Ordering::Relaxed)
    }

    pub fn set_close_to_tray_on_close(&self, enabled: bool) {
        self.close_to_tray_on_close.store(enabled, Ordering::Relaxed);
    }

    fn set_quitting(&self) {
        self.quitting.store(true, Ordering::Relaxed);
    }

    fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::Relaxed)
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

    let state_for_menu = state.clone();
    let main_window_for_menu = main_window.clone();
    let app_handle_for_menu = app_handle.clone();

    let main_window_for_tray = main_window.clone();

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("TauriTavern")
        .menu(&menu)
        .on_menu_event(move |_app, event| match event.id().as_ref() {
            MENU_SHOW_ID => {
                main_window_for_menu
                    .show()
                    .and_then(|_| main_window_for_menu.set_focus())
                    .expect("Failed to show main window from tray menu");
            }
            MENU_EXIT_ID => {
                state_for_menu.set_quitting();
                app_handle_for_menu.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(move |_tray, event| match event {
            TrayIconEvent::DoubleClick { button, .. } if button == MouseButton::Left => {
                main_window_for_tray
                    .show()
                    .and_then(|_| main_window_for_tray.set_focus())
                    .expect("Failed to show main window from tray icon");
            }
            _ => {}
        })
        .build(app_handle)?;

    let state_for_close = state.clone();
    let main_window_for_close = main_window.clone();
    main_window.on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };

        if state_for_close.is_quitting() || !state_for_close.close_to_tray_on_close() {
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
