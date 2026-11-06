//! Window management. Every window is a SvelteKit route (`src/routes/<label>`).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::cli::Action;

pub const MAIN: &str = "main";
pub const OVERLAY: &str = "overlay";
const RECORDER: &str = "recorder";
const SETTINGS: &str = "settings";

pub fn dispatch(app: &AppHandle, action: Action) {
    let result = match action {
        Action::Screenshot => {
            crate::capture::start(app, false);
            Ok(())
        }
        Action::Record => {
            open(app, RECORDER, |b| b.title("FrameNab – Recording").inner_size(720.0, 480.0).always_on_top(true))
        }
        Action::Editor => {
            show_main(app);
            Ok(())
        }
        Action::Settings => open(app, SETTINGS, |b| b.title("FrameNab – Settings").inner_size(640.0, 520.0)),
    };
    if let Err(err) = result {
        tracing::error!(?action, %err, "failed to open window");
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
}

pub fn main_visible(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN).and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

pub fn overlay(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(OVERLAY)
}

pub fn focus_overlay(app: &AppHandle) {
    if let Some(w) = overlay(app) {
        let _ = w.set_focus();
    }
}

pub fn close_overlay(app: &AppHandle) {
    if let Some(w) = overlay(app) {
        let _ = w.close();
    }
}

/// Screenshot overlay: borderless, always on top, covering the whole screen. It starts hidden
/// and shows itself once the frozen screen image is drawn, to avoid a white flash.
pub fn open_overlay(app: &AppHandle) -> tauri::Result<()> {
    close_overlay(app);
    let mut builder = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App(OVERLAY.into()));
    // Start at the full monitor size: on Linux, WebKitGTK does not always resize its viewport
    // when the window switches to fullscreen afterwards.
    // Wayland has no notion of a primary monitor, so fall back to the first one.
    let monitor = app.primary_monitor().ok().flatten().or_else(|| app.available_monitors().ok()?.into_iter().next());
    if let Some(monitor) = monitor {
        let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
        let pos = monitor.position().to_logical::<f64>(monitor.scale_factor());
        tracing::debug!(?size, ?pos, scale = monitor.scale_factor(), "overlay monitor");
        builder = builder.inner_size(size.width, size.height).position(pos.x, pos.y);
    } else {
        tracing::warn!("no monitor information available for the overlay");
    }
    builder
        .title("FrameNab – Screenshot")
        .decorations(false)
        .always_on_top(true)
        // Keep a taskbar/dock entry: the main window may be hidden while the overlay is open,
        // and FrameNab should not look as if it had quit.
        .resizable(false)
        .fullscreen(true)
        .visible(false)
        .build()?;
    Ok(())
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
