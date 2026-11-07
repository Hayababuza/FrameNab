//! Window management. Every window is a SvelteKit route (`src/routes/<label>`).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::cli::Action;

pub const MAIN: &str = "main";
pub const OVERLAY: &str = "overlay";
pub const RECORDER: &str = "recorder";
const SETTINGS: &str = "settings";

pub fn dispatch(app: &AppHandle, action: Action) {
    let result = match action {
        Action::Screenshot => {
            crate::capture::start(app, false, crate::capture::CaptureMode::Screenshot);
            Ok(())
        }
        Action::Record => {
            record(app, false);
            Ok(())
        }
        Action::RecordScreen => {
            record_screen(app);
            Ok(())
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

/// "Record" from the tray, a shortcut or the CLI: stops a running recording, brings an open
/// recorder to the front, or starts choosing the area of a new one.
pub fn record(app: &AppHandle, hide_main: bool) {
    if crate::recorder::is_recording(app) {
        crate::recorder::request_stop(app);
    } else if let Some(w) = app.get_webview_window(RECORDER) {
        let _ = w.set_focus();
    } else {
        crate::capture::start(app, hide_main, crate::capture::CaptureMode::Record);
    }
}

/// Opens the recorder for the whole primary monitor, without choosing an area first.
fn record_screen(app: &AppHandle) {
    if crate::recorder::is_recording(app) {
        return crate::recorder::request_stop(app);
    }
    let monitor = app.primary_monitor().ok().flatten().or_else(|| app.available_monitors().ok()?.into_iter().next());
    let Some(monitor) = monitor else { return };
    let size = monitor.size();
    let region = crate::recorder::Region { x: 0, y: 0, w: size.width, h: size.height };
    crate::recorder::open(app, Some(region), (size.width, size.height), None, false);
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

/// Recorder: borderless, transparent and fullscreen; the page draws the frame and controls and
/// makes everything else click-through. Starts hidden until the page has laid itself out.
pub fn open_recorder(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(RECORDER) {
        let _ = w.close();
    }
    let mut builder = WebviewWindowBuilder::new(app, RECORDER, WebviewUrl::App(RECORDER.into()));
    let monitor = app.primary_monitor().ok().flatten().or_else(|| app.available_monitors().ok()?.into_iter().next());
    if let Some(monitor) = monitor {
        let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
        let pos = monitor.position().to_logical::<f64>(monitor.scale_factor());
        builder = builder.inner_size(size.width, size.height).position(pos.x, pos.y);
    }
    builder
        .title("FrameNab – Recording")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(false)
        .fullscreen(true)
        .visible(false)
        .build()?;
    Ok(())
}

/// Works around a tao bug on Wayland: tao wraps the GTK header bar in an `EventBox` whose input
/// window sits *above* its child, so the minimize/maximize/close buttons ignore clicks until GTK
/// re-allocates the header (e.g. after maximizing once). Putting the event box below its child
/// lets the buttons receive clicks; dragging on empty header space still reaches the event box.
#[cfg(target_os = "linux")]
pub fn fix_titlebar(window: &WebviewWindow) {
    use gtk::prelude::*;

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(gtk_window) = target.gtk_window() else { return };
        let Some(titlebar) = gtk_window.titlebar() else { return };
        if let Ok(event_box) = titlebar.downcast::<gtk::EventBox>()
            && event_box.is_above_child()
        {
            event_box.set_above_child(false);
            tracing::debug!(window = target.label(), "titlebar event box moved below the header bar");
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn fix_titlebar(_window: &WebviewWindow) {}

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
    let window = configure(WebviewWindowBuilder::new(app, label, WebviewUrl::App(label.into()))).build()?;
    fix_titlebar(&window);
    Ok(())
}
