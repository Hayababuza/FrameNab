use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Result};

use crate::cli::Action;
use crate::i18n::t;
use crate::windows;

pub fn create(app: &AppHandle) -> Result<()> {
    let item = |id: &str, key: &str| MenuItem::with_id(app, id, t(key), true, None::<&str>);
    let menu = Menu::with_items(
        app,
        &[
            &item("screenshot", "tray.screenshot")?,
            &item("record", "tray.record")?,
            &item("instant", "tray.instant")?,
            &item("editor", "tray.editor")?,
            &PredefinedMenuItem::separator(app)?,
            &item("settings", "tray.settings")?,
            &item("quit", "tray.quit")?,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("main").tooltip("FrameNab").menu(&menu).on_menu_event(|app, event| {
        match event.id.as_ref() {
            "screenshot" => windows::dispatch(app, Action::Screenshot),
            "record" => windows::dispatch(app, Action::Record),
            "instant" => windows::dispatch(app, Action::RecordNow),
            "editor" => windows::dispatch(app, Action::Editor),
            "settings" => windows::dispatch(app, Action::Settings),
            "quit" => app.exit(0),
            _ => {}
        }
    });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
