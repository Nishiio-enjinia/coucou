// Notification-area icon: Open, Settings, Pause, Quit.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

use crate::island::WINDOW_LABEL;

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", crate::i18n::t("tray.open"), true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", crate::i18n::t("tray.settings"), true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", crate::i18n::t("tray.pause"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", crate::i18n::t("tray.quit"), true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&open, &sep1, &settings, &pause, &sep2, &quit])
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app)?;

    let mut builder = TrayIconBuilder::with_id("coucou")
        .tooltip("Coucou")
        .menu(&menu)
        .on_menu_event(|app: &AppHandle, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "settings" => crate::show_settings_window(app),
            id => {
                let _ = app.emit_to(WINDOW_LABEL, "tray", id.to_string());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

/// Rewrites the tray labels and the settings window title after a language change.
pub fn refresh(app: &AppHandle) {
    if let Ok(menu) = menu(app) {
        if let Some(icon) = app.tray_by_id("coucou") {
            let _ = icon.set_menu(Some(menu));
        }
    }
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.set_title(crate::i18n::t("settings.windowTitle"));
    }
}
