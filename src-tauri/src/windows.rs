//! Window management. Every window is a SvelteKit route (`src/routes/<label>`).

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
            open(app, RECORDER, |b| b.title("FrameNab – Recording").inner_size(720.0, 480.0).always_on_top(true))
        }
        Action::Editor => show_main(app),
        Action::Settings => open(app, SETTINGS, |b| b.title("FrameNab – Settings").inner_size(640.0, 520.0)),
    };
    if let Err(err) = result {
        tracing::error!(?action, %err, "Failed to open window");
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

/// Screenshot overlay: borderless, always on top, covering the whole screen.
/// In phase 1 it shows the frozen screen image for selecting/annotating.
fn open_overlay(app: &AppHandle) -> tauri::Result<()> {
    open(app, OVERLAY, |b| {
        b.title("FrameNab – Screenshot").decorations(false).always_on_top(true).skip_taskbar(true).fullscreen(true)
    })
}

/// Opens a window, or brings an already open one to the front.
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
