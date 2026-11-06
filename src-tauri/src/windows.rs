//! Fensterverwaltung. Jedes Fenster ist eine SvelteKit-Route (`src/routes/<label>`).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::cli::Action;

pub const MAIN: &str = "main";
const OVERLAY: &str = "overlay";
const RECORDER: &str = "recorder";
const SETTINGS: &str = "settings";

pub fn dispatch(app: &AppHandle, action: Action) {
    let result = match action {
        Action::Screenshot => open_overlay(app),
        Action::Record => {
            open(app, RECORDER, |b| b.title("FrameNab – Aufnahme").inner_size(720.0, 480.0).always_on_top(true))
        }
        Action::Editor => show_main(app),
        Action::Settings => open(app, SETTINGS, |b| b.title("FrameNab – Einstellungen").inner_size(640.0, 520.0)),
    };
    if let Err(err) = result {
        tracing::error!(?action, %err, "Fenster konnte nicht geöffnet werden");
    }
}

fn show_main(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(MAIN) {
        w.show()?;
        w.unminimize()?;
        w.set_focus()?;
    }
    Ok(())
}

pub fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
}

/// Screenshot-Overlay: randlos, im Vordergrund, über den ganzen Bildschirm.
/// Phase 1 zeigt hier das eingefrorene Bildschirmbild zum Auswählen/Annotieren.
fn open_overlay(app: &AppHandle) -> tauri::Result<()> {
    open(app, OVERLAY, |b| {
        b.title("FrameNab – Screenshot").decorations(false).always_on_top(true).skip_taskbar(true).fullscreen(true)
    })
}

/// Öffnet ein Fenster bzw. holt ein bereits offenes nach vorn.
fn open<F>(app: &AppHandle, label: &str, configure: F) -> tauri::Result<()>
where
    F: for<'a> FnOnce(
        WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
    ) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
{
    if let Some(w) = app.get_webview_window(label) {
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    configure(WebviewWindowBuilder::new(app, label, WebviewUrl::App(label.into()))).build()?;
    Ok(())
}
