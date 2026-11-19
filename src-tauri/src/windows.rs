//! Window management. Every window is a SvelteKit route (`src/routes/<label>`).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::cli::Action;

pub const MAIN: &str = "main";
pub const OVERLAY: &str = "overlay";
pub const RECORDER: &str = "recorder";
const SETTINGS: &str = "settings";

/// A window that was hidden for a capture (so it is not in the picture) and should come back
/// afterwards. The duty is handed along: overlay → recorder/pin → editor.
#[derive(Default)]
pub struct Restore(std::sync::Mutex<Option<String>>);

impl Restore {
    pub fn set(&self, label: Option<String>) {
        if label.is_some() {
            *self.0.lock().expect("restore lock") = label;
        }
    }

    pub fn take(&self) -> Option<String> {
        self.0.lock().expect("restore lock").take()
    }

    /// Shows the remembered window, if any.
    pub fn restore(&self, app: &AppHandle) {
        if let Some(label) = self.take() {
            show_window(app, &label);
        }
    }
}

/// Brings `window` to the front with the keyboard focus.
///
/// X11/XWayland (GNOME): opened from the tray, a second launch or a shortcut, there is no recent
/// input event of FrameNapper's own, so GNOME's focus-stealing prevention only shows a "FrameNapper is
/// ready" notification instead of the window. Presenting it with the X server's current time
/// counts as a fresh request.
pub fn focus(window: &WebviewWindow) {
    #[cfg(target_os = "linux")]
    {
        use gtk::glib::Cast;
        use gtk::prelude::*;

        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            let Ok(gtk_window) = target.gtk_window() else { return };
            let x11 = gtk_window.window().and_then(|w| w.downcast::<gdkx11::X11Window>().ok());
            match x11 {
                Some(x11) => gtk_window.present_with_time(gdkx11::functions::x11_get_server_time(&x11)),
                None => gtk_window.present(),
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window.set_focus();
    }
}

pub fn show_window(app: &AppHandle, label: &str) {
    if label == MAIN {
        return show_main(app);
    }
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        focus(&w);
    }
}

/// Hides `label` if it is visible; returns whether it was.
pub fn hide_window(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label).is_some_and(|w| w.is_visible().unwrap_or(false) && w.hide().is_ok())
}

/// Runs `action` from an event handler (tray menu, global shortcut, second launch).
///
/// Creating a window inside an event handler deadlocks on Windows (WebView2, wry#583): the window
/// appears but stays blank and does not react, not even to closing. So the action runs in an
/// async task instead, like the `async` commands do.
pub fn dispatch(app: &AppHandle, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(&app, action) });
}

/// Runs `action` right away. Only call this from an `async` command or another async task.
pub fn run(app: &AppHandle, action: Action) {
    let result = match action {
        Action::Screenshot => {
            crate::capture::start(app, None, crate::capture::CaptureMode::Screenshot);
            Ok(())
        }
        Action::Record => {
            record(app, None);
            Ok(())
        }
        Action::RecordScreen => {
            record_screen(app);
            Ok(())
        }
        Action::RecordNow => {
            instant_gif(app, None);
            Ok(())
        }
        Action::Editor => {
            show_main(app);
            Ok(())
        }
        Action::Settings => open(app, SETTINGS, |b| b.title("FrameNapper – Settings").inner_size(640.0, 520.0)),
    };
    if let Err(err) = result {
        tracing::error!(?action, %err, "failed to open window");
    }
}

/// "Record" from the tray, a shortcut or the CLI: stops a running recording, brings an open
/// recorder to the front, or starts choosing the area of a new one.
/// `hide`: the window the request came from; it is hidden during the capture and restored.
pub fn record(app: &AppHandle, hide: Option<String>) {
    if crate::recorder::is_recording(app) {
        crate::recorder::request_stop(app);
    } else if let Some(w) = app.get_webview_window(RECORDER) {
        focus(&w);
    } else {
        crate::capture::start(app, hide, crate::capture::CaptureMode::Record);
    }
}

/// Instant GIF (tray, shortcut, CLI, start page): choose an area, it records right away and is
/// saved and copied when stopped. Stops a running recording instead.
pub fn instant_gif(app: &AppHandle, hide: Option<String>) {
    if crate::recorder::is_recording(app) {
        crate::recorder::request_stop(app);
    } else if let Some(w) = app.get_webview_window(RECORDER) {
        focus(&w);
    } else {
        crate::capture::start(app, hide, crate::capture::CaptureMode::Instant);
    }
}

/// Opens the recorder for the whole primary monitor, without choosing an area first.
fn record_screen(app: &AppHandle) {
    if crate::recorder::is_recording(app) {
        return crate::recorder::request_stop(app);
    }
    let Some(monitor) = monitor_at(app, None) else { return };
    let region = crate::recorder::Region { x: 0, y: 0, w: monitor.width, h: monitor.height };
    let portal = framenapper_capture::Platform::detect().requires_portal();
    crate::recorder::open(
        app,
        crate::recorder::Open {
            region: Some(region),
            size: (monitor.width, monitor.height),
            origin: if portal { (0, 0) } else { (monitor.x, monitor.y) },
            background: None,
            restore: None,
            placement: Some(monitor),
            instant: None,
        },
    );
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        focus(&w);
    }
}

pub fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
}

pub fn overlay(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(OVERLAY)
}

pub fn focus_overlay(app: &AppHandle) {
    if let Some(w) = overlay(app) {
        focus(&w);
    }
}

pub fn close_overlay(app: &AppHandle) {
    if let Some(w) = overlay(app) {
        let _ = w.close();
    }
}

/// Where the overlay and the recorder go, in physical pixels of the desktop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    /// One window across all monitors (Windows with several monitors) instead of a fullscreen
    /// window on one monitor.
    pub span: bool,
}

impl Placement {
    pub fn of(monitor: &tauri::Monitor) -> Self {
        let (pos, size) = (monitor.position(), monitor.size());
        Self { x: pos.x, y: pos.y, width: size.width, height: size.height, scale: monitor.scale_factor(), span: false }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width as i32 && y < self.y + self.height as i32
    }
}

/// The overlay/recorder placement: on Windows with several monitors one window across all of them
/// (so an area can be chosen on any monitor, or across them); elsewhere fullscreen on the monitor
/// under the mouse (`at`: a desktop point to use instead), else the primary one.
///
/// Spanning is Windows-only: macOS keeps windows on one display, X11 window managers and Wayland
/// do not reliably allow a window across monitors.
pub fn placement(app: &AppHandle, at: Option<(i32, i32)>) -> Option<Placement> {
    let monitors = app.available_monitors().ok().filter(|m| !m.is_empty())?;
    if cfg!(windows) && monitors.len() > 1 {
        let all: Vec<Placement> = monitors.iter().map(Placement::of).collect();
        let min_x = all.iter().map(|p| p.x).min()?;
        let min_y = all.iter().map(|p| p.y).min()?;
        let max_x = all.iter().map(|p| p.x + p.width as i32).max()?;
        let max_y = all.iter().map(|p| p.y + p.height as i32).max()?;
        let scale = app.primary_monitor().ok().flatten().map_or(1.0, |m| m.scale_factor());
        return Some(Placement {
            x: min_x,
            y: min_y,
            width: (max_x - min_x) as u32,
            height: (max_y - min_y) as u32,
            scale,
            span: true,
        });
    }
    monitor_at(app, at)
}

/// The monitor at `at` (a desktop point), else the one under the mouse, else the primary one.
pub fn monitor_at(app: &AppHandle, at: Option<(i32, i32)>) -> Option<Placement> {
    let all: Vec<Placement> = app.available_monitors().ok()?.iter().map(Placement::of).collect();
    let point = at.or_else(|| app.cursor_position().ok().map(|p| (p.x as i32, p.y as i32)));
    let under = point.and_then(|(x, y)| all.iter().find(|p| p.contains(x, y)).copied());
    let primary = app.primary_monitor().ok().flatten().map(|m| Placement::of(&m));
    under.or(primary).or(all.first().copied())
}

/// Sizes and positions a window built hidden by [`builder_at`] to its placement.
fn place(window: &WebviewWindow, placement: &Placement) -> tauri::Result<()> {
    if placement.span {
        // Physical units: with mixed DPI the logical size depends on the monitor. Position first:
        // moving to another monitor may change the window's scale and size.
        window.set_position(tauri::PhysicalPosition::new(placement.x, placement.y))?;
        window.set_size(tauri::PhysicalSize::new(placement.width, placement.height))?;
    }
    Ok(())
}

/// Builder for a borderless, always-on-top window on `placement`: fullscreen on its monitor, or
/// (spanning) a plain window across the desktop, sized after it was built (see [`place`]).
fn builder_at<'a>(
    app: &'a AppHandle,
    label: &str,
    placement: Option<&Placement>,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App(label.into()))
        .decorations(false)
        .always_on_top(true)
        .resizable(false)
        .visible(false);
    match placement {
        Some(p) if p.span => {
            builder = builder.shadow(false);
        }
        Some(p) => {
            // Start at the full monitor size: on Linux, WebKitGTK does not always resize its
            // viewport when the window switches to fullscreen afterwards.
            let size = tauri::PhysicalSize::new(p.width, p.height).to_logical::<f64>(p.scale);
            let pos = tauri::PhysicalPosition::new(p.x, p.y).to_logical::<f64>(p.scale);
            tracing::debug!(?size, ?pos, scale = p.scale, "window monitor");
            builder = builder.inner_size(size.width, size.height).position(pos.x, pos.y).fullscreen(true);
        }
        None => {
            tracing::warn!("no monitor information available for the window");
            builder = builder.fullscreen(true);
        }
    }
    builder
}

/// Screenshot overlay: borderless, always on top, covering the screen(s). It starts hidden and
/// shows itself once the frozen screen image is drawn, to avoid a white flash.
// Keeps a taskbar/dock entry: the main window may be hidden while the overlay is open, and
// FrameNapper should not look as if it had quit.
pub fn open_overlay(app: &AppHandle, placement: Option<&Placement>) -> tauri::Result<()> {
    close_overlay(app);
    let window = builder_at(app, OVERLAY, placement).title("FrameNapper – Screenshot").build()?;
    if let Some(p) = placement {
        place(&window, p)?;
    }
    Ok(())
}

/// Recorder: borderless, transparent and covering the screen(s); the page draws the frame and
/// controls and makes everything else click-through. Starts hidden until the page has laid
/// itself out.
pub fn open_recorder(app: &AppHandle, placement: Option<&Placement>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(RECORDER) {
        let _ = w.close();
    }
    let window = builder_at(app, RECORDER, placement).title("FrameNapper – Recording").transparent(true).build()?;
    if let Some(p) = placement {
        place(&window, p)?;
    }
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
        focus(&w);
        return Ok(());
    }
    let window = configure(WebviewWindowBuilder::new(app, label, WebviewUrl::App(label.into()))).build()?;
    fix_titlebar(&window);
    focus(&window);
    Ok(())
}
