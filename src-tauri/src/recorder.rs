//! Screen recording in the app: after the area was chosen in the overlay (record mode), a
//! borderless, transparent, fullscreen recorder window shows a frame around the area and the
//! controls. Only the controls take mouse input; everything else is click-through, so the
//! recorded apps can be used while recording.
//!
//! Flow (commands called by `src/routes/recorder`):
//! `recorder_info` → `recorder_prepare` (opens the stream; on Wayland the screen-sharing dialog
//! may appear) → countdown in the UI → `recorder_start` → `recorder_pause`/`recorder_resume` →
//! `recorder_stop` (writes `project.json`, returns a summary).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use framenab_capture::{CaptureTarget, Crop, FrameStream, Recording, StreamOptions, record};
use framenab_core::recorder::ProjectRecorder;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_store::StoreExt;

use crate::windows;

/// Store key for the Wayland screen-sharing restore token.
const TOKEN_KEY: &str = "screencastRestoreToken";

/// Area to record, in pixels of the screenshot the overlay showed (= the pixels of the recorder
/// window, which covers the same monitor(s)). Add [`Setup`]'s origin for desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    /// `None` when recording a single window chosen in the system dialog (Wayland).
    region: Option<Region>,
    image_width: u32,
    image_height: u32,
    /// The frozen screen image from the area selection is available as background
    /// (`framenab://localhost/recorder-background`). Used on Wayland, where a fullscreen window
    /// cannot be transparent: GNOME puts black behind fullscreen windows.
    background: bool,
    /// Wayland: the window has to shrink to a compact bar while recording.
    compact: bool,
    /// The window covers all monitors (no fullscreen request).
    span: bool,
    /// Start right away (quick recording by shortcut), without waiting for the start button.
    autostart: bool,
    /// Desktop position of the region's coordinate origin, in physical pixels.
    #[serde(skip)]
    origin: (i32, i32),
}

#[derive(Default)]
struct Inner {
    setup: Option<Setup>,
    stream: Option<Box<dyn FrameStream>>,
    recording: Option<Recording<ProjectRecorder>>,
    project_dir: Option<PathBuf>,
    /// Records clicks and keys while recording (dropping it stops it).
    input: Option<framenab_capture::input::InputListener>,
    /// Show the pressed keys in the result (chosen in the recorder bar).
    show_keys: bool,
}

#[derive(Default)]
pub struct RecorderState {
    inner: Mutex<Inner>,
    background: Mutex<Option<Arc<RgbaImage>>>,
    /// The main window was hidden for the capture; show it again when the recorder closes.
    restore: windows::Restore,
    /// Click-through: only these rects (logical pixels) take mouse input (non-Linux).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    interactive: Arc<Mutex<Vec<InputRect>>>,
}

impl RecorderState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("recorder lock")
    }

    /// The frozen screen image shown behind the frame before recording (Wayland).
    pub fn background(&self) -> Option<Arc<RgbaImage>> {
        self.background.lock().expect("background lock").clone()
    }
}

/// What the recorder window is opened with.
pub struct Open {
    /// `None`: a window chosen later in the system dialog (Wayland).
    pub region: Option<Region>,
    /// Size of the screenshot the region refers to (= the window's size in physical pixels).
    pub size: (u32, u32),
    /// Desktop position of the region's coordinate origin.
    pub origin: (i32, i32),
    /// The frozen screen image of the area selection, if there was one.
    pub background: Option<Arc<RgbaImage>>,
    /// Window to show again when the recorder closes.
    pub restore: Option<String>,
    pub placement: Option<windows::Placement>,
    pub autostart: bool,
}

/// Opens the recorder window.
pub fn open(app: &AppHandle, open: Open) {
    let state = app.state::<RecorderState>();
    // Native Wayland windows cannot be transparent in fullscreen nor stay on top.
    let compact = crate::display::native_wayland();
    let background = open.background.filter(|_| compact);
    let span = open.placement.is_some_and(|p| p.span);
    {
        let mut inner = state.lock();
        *inner = Inner::default();
        inner.setup = Some(Setup {
            region: open.region,
            image_width: open.size.0,
            image_height: open.size.1,
            background: background.is_some(),
            compact,
            span,
            autostart: open.autostart,
            origin: open.origin,
        });
    }
    *state.background.lock().expect("background lock") = background;
    state.restore.set(open.restore);
    if let Err(err) = windows::open_recorder(app, open.placement.as_ref()) {
        tracing::error!(%err, "failed to open the recorder window");
    }
}

/// Settings key: the last recorded area, in desktop pixels (relative to the monitor on Wayland).
const LAST_AREA_KEY: &str = "lastRecordArea";

/// Quick recording (shortcut, `framenab record-now`): records the last recorded area again, or
/// the whole monitor under the mouse, and starts right away (after the countdown). Stops a
/// running recording instead.
pub fn record_now(app: &AppHandle) {
    if is_recording(app) {
        return request_stop(app);
    }
    if let Some(w) = app.get_webview_window(windows::RECORDER) {
        let _ = w.set_focus();
        return;
    }
    let last: Option<Region> =
        app.store("settings.json").ok().and_then(|s| s.get(LAST_AREA_KEY)).and_then(|v| serde_json::from_value(v).ok());
    let portal = framenab_capture::Platform::detect().requires_portal();
    let center = last.map(|r| (r.x + r.w as i32 / 2, r.y + r.h as i32 / 2));
    let Some(placement) = windows::placement(app, center.filter(|_| !portal)) else {
        tracing::warn!("no monitor found for a quick recording");
        return;
    };
    // Region coordinates are relative to the window; on Wayland the stored area already is.
    let origin = if portal { (0, 0) } else { (placement.x, placement.y) };
    let fits = |r: &Region| {
        let (x, y) = (r.x - origin.0, r.y - origin.1);
        x >= 0 && y >= 0 && x as u32 + r.w <= placement.width && y as u32 + r.h <= placement.height
    };
    let region = match last.filter(fits) {
        Some(r) => Region { x: r.x - origin.0, y: r.y - origin.1, w: r.w, h: r.h },
        None => {
            // The whole monitor under the mouse (within a window that spans all of them).
            let monitor = windows::monitor_at(app, None).filter(|_| placement.span && !portal);
            match monitor {
                Some(m) => Region { x: m.x - origin.0, y: m.y - origin.1, w: m.width, h: m.height },
                None => Region { x: 0, y: 0, w: placement.width, h: placement.height },
            }
        }
    };
    tracing::info!(?region, ?placement, "quick recording");
    open(
        app,
        Open {
            region: Some(region),
            size: (placement.width, placement.height),
            origin,
            background: None,
            restore: None,
            placement: Some(placement),
            autostart: true,
        },
    );
}

/// Is a recording running (used by the tray/shortcut to stop it)?
pub fn is_recording(app: &AppHandle) -> bool {
    app.state::<RecorderState>().lock().recording.is_some()
}

/// Asks the recorder window to stop (it updates its UI and calls `recorder_stop`).
pub fn request_stop(app: &AppHandle) {
    let _ = app.emit_to(windows::RECORDER, "recorder-stop-requested", ());
}

/// Called when the recorder window is destroyed: end everything that still runs.
pub fn cleanup(app: &AppHandle) {
    let state = app.state::<RecorderState>();
    let (stream, recording, dir) = {
        let mut inner = state.lock();
        (inner.stream.take(), inner.recording.take(), inner.project_dir.take())
    };
    if let Some(stream) = stream {
        stream.stop();
    }
    let show_keys = {
        let mut inner = state.lock();
        inner.input.take();
        inner.show_keys
    };
    if let Some(recording) = recording {
        // Closed while recording: keep what was recorded.
        let (sink, end, _, input) = recording.stop();
        match (sink.finish(end), dir) {
            (Err(err), _) => tracing::warn!(%err, "could not finish the recording"),
            (Ok((project, _)), Some(dir)) => {
                if let Err(err) = save_input(&dir, project, &input, show_keys) {
                    tracing::warn!(%err, "could not save the recorded input");
                }
                tracing::info!(path = %dir.display(), "recording kept after the recorder was closed");
            }
            (Ok(_), None) => {}
        }
    }
    state.background.lock().expect("background lock").take();
    state.restore.restore(app);
}

#[tauri::command]
pub fn recorder_info(app: AppHandle) -> Option<Setup> {
    app.state::<RecorderState>().lock().setup.clone()
}

/// Opens the capture stream. On Wayland this may show the screen-sharing dialog (once for
/// monitors: the choice is remembered in the settings).
#[tauri::command]
pub async fn recorder_prepare(app: AppHandle, cursor: bool, fps: u32) -> Result<(), String> {
    let setup = app.state::<RecorderState>().lock().setup.clone().ok_or("recorder is not set up")?;
    let store = app.store("settings.json").map_err(|e| e.to_string())?;
    let restore_token = store.get(TOKEN_KEY).and_then(|v| v.as_str().map(str::to_owned));
    let target = match setup.region {
        Some(r) => CaptureTarget::Region { x: r.x + setup.origin.0, y: r.y + setup.origin.1, width: r.w, height: r.h },
        None => CaptureTarget::Window(0),
    };

    let stream = tauri::async_runtime::spawn_blocking(move || {
        framenab_capture::backend().open_stream(&target, &StreamOptions { cursor, restore_token, fps })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    if setup.region.is_some()
        && let Some(token) = stream.restore_token()
    {
        store.set(TOKEN_KEY, token);
    }
    let old = app.state::<RecorderState>().lock().stream.replace(stream);
    if let Some(old) = old {
        old.stop();
    }
    Ok(())
}

/// Starts sampling the prepared stream into a new project folder. `show_keys`: show the pressed
/// keys in the result (they are recorded either way, if allowed, to switch on in the editor).
#[tauri::command]
pub fn recorder_start(app: AppHandle, fps: u32, show_keys: bool) -> Result<(), String> {
    let state = app.state::<RecorderState>();
    let mut inner = state.lock();
    let stream = inner.stream.take().ok_or("the stream is not prepared")?;
    let origin = stream.origin();
    let setup = inner.setup.clone();
    let area = setup.as_ref().and_then(|s| s.region.map(|r| (r, s.origin)));
    let crop = area.map(|(r, o)| Crop { x: r.x + o.0 - origin.0, y: r.y + o.1 - origin.1, width: r.w, height: r.h });
    // Remembered for the quick recording.
    if let Some((r, o)) = area
        && let Ok(store) = app.store("settings.json")
        && let Ok(value) = serde_json::to_value(Region { x: r.x + o.0, y: r.y + o.1, ..r })
    {
        store.set(LAST_AREA_KEY, value);
    }
    inner.show_keys = show_keys;
    let dir = recordings_dir(&app)
        .join(format!("{}.framenab", framenab_export::still::file_name_now(framenab_export::still::DEFAULT_TEMPLATE)));
    let sink = ProjectRecorder::create(&dir).map_err(|e| e.to_string())?;
    let recording = record(stream, crop, fps, sink);
    let record_input =
        app.store("settings.json").ok().and_then(|s| s.get(INPUT_KEY)).and_then(|v| v.as_bool()).unwrap_or(true);
    inner.input = if record_input { framenab_capture::input::listen(recording.input_recorder(), origin) } else { None };
    inner.recording = Some(recording);
    inner.project_dir = Some(dir.clone());
    tracing::info!(path = %dir.display(), fps, ?crop, clicks_and_keys = inner.input.is_some(), "recording started");
    Ok(())
}

#[tauri::command]
pub fn recorder_pause(app: AppHandle, paused: bool) {
    if let Some(recording) = &app.state::<RecorderState>().lock().recording {
        if paused { recording.pause() } else { recording.resume() }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    recording: bool,
    paused: bool,
    elapsed_ms: u64,
    error: Option<String>,
}

#[tauri::command]
pub fn recorder_status(app: AppHandle) -> Status {
    let state = app.state::<RecorderState>();
    let inner = state.lock();
    match &inner.recording {
        Some(r) => Status { recording: true, paused: r.is_paused(), elapsed_ms: r.elapsed_ms(), error: r.error() },
        None => Status { recording: false, paused: false, elapsed_ms: 0, error: None },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    path: String,
    frames: usize,
    duration_ms: u64,
    width: u32,
    height: u32,
    dropped: u64,
    /// Saved right away (setting "after recording: save"): where the file is.
    saved: Option<String>,
}

/// Settings key: what happens after a recording, `"editor"` (default) or `"save"` (save in the
/// format from the settings, copy, done).
const AFTER_KEY: &str = "afterRecording";

#[tauri::command]
pub async fn recorder_stop(app: AppHandle) -> Result<Summary, String> {
    let (recording, dir, show_keys) = {
        let state = app.state::<RecorderState>();
        let mut inner = state.lock();
        inner.input.take();
        (
            inner.recording.take().ok_or("not recording")?,
            inner.project_dir.clone().ok_or("no project folder")?,
            inner.show_keys,
        )
    };
    let target = dir.clone();
    let (project, stats) = tauri::async_runtime::spawn_blocking(move || {
        let (sink, end, _, input) = recording.stop();
        let (project, stats) = sink.finish(end).map_err(|e| e.to_string())?;
        let project = save_input(&target, project, &input, show_keys)?;
        Ok::<_, String>((project, stats))
    })
    .await
    .map_err(|e| e.to_string())??;
    tracing::info!(path = %dir.display(), frames = stats.frames, dropped = stats.dropped, "recording finished");
    let restore = app.state::<RecorderState>().restore.take();
    let direct = app.store("settings.json").ok().and_then(|s| s.get(AFTER_KEY)).is_some_and(|v| v == "save");
    let saved = if direct {
        match save_directly(&app, &dir, &project).await {
            Ok(path) => Some(path.display().to_string()),
            Err(err) => {
                tracing::error!(%err, "saving the recording directly failed");
                use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                app.dialog()
                    .message(format!("{}\n\n{err}", crate::i18n::t("save.recordingFailed")))
                    .title("FrameNab")
                    .kind(MessageDialogKind::Error)
                    .show(|_| {});
                None
            }
        }
    } else {
        None
    };
    if saved.is_some() {
        if let Some(label) = restore {
            windows::show_window(&app, &label);
        }
    } else {
        // Continue in the editor; it takes over bringing the main window back.
        crate::editor::open(&app, dir.clone(), restore)?;
    }
    Ok(Summary {
        path: dir.display().to_string(),
        frames: project.frames.len(),
        duration_ms: project.duration_ms(),
        width: project.width,
        height: project.height,
        dropped: stats.dropped,
        saved,
    })
}

/// Saves a finished recording into the save folder (file name from the template, format and
/// options from the settings) and copies it; the project stays in the recordings for later edits.
async fn save_directly(
    app: &AppHandle,
    dir: &std::path::Path,
    project: &framenab_core::Project,
) -> Result<PathBuf, String> {
    use framenab_export::animation::{ExportControl, export_animation};
    use framenab_export::still::{file_name_now, unique_path};

    let (format, options) = crate::editor::saved_choice(app);
    let config = crate::settings::save_settings(app);
    let path = unique_path(&config.directory, &file_name_now(&config.template), format.extension());
    let (dir, project, target) = (dir.to_owned(), project.clone(), path.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let store = framenab_core::FrameStore::create(&dir).map_err(|e| e.to_string())?;
        export_animation(&store, &project, &target, format, &options, &ExportControl::default())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    crate::editor::announce_saved(app, &path).await;
    Ok(path)
}

/// Settings key: record clicks and keys (default on).
const INPUT_KEY: &str = "recordInput";

/// Saves the recorded input next to the frames and turns on the effects that have data.
fn save_input(
    dir: &std::path::Path,
    mut project: framenab_core::Project,
    input: &framenab_core::InputEvents,
    show_keys: bool,
) -> Result<framenab_core::Project, String> {
    let store = framenab_core::FrameStore::create(dir).map_err(|e| e.to_string())?;
    store.save_input(input).map_err(|e| e.to_string())?;
    project.effects.cursor = !input.cursor.is_empty();
    project.effects.clicks = !input.clicks.is_empty();
    project.effects.keys = show_keys && !input.keys.is_empty();
    store.save_project(&project).map_err(|e| e.to_string())?;
    Ok(project)
}

/// Whether clicks and keys can be recorded here (Linux needs the `input` group).
#[tauri::command]
pub fn input_support() -> framenab_capture::input::InputSupport {
    framenab_capture::input::support()
}

/// `<app data>/recordings`
/// Folder of the recording that is running right now.
pub fn recording_dir(app: &AppHandle) -> Option<PathBuf> {
    app.state::<RecorderState>().lock().project_dir.clone()
}

pub fn recordings_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("framenab")).join("recordings")
}

// ---- click-through ----------------------------------------------------------------------------

/// A rect in logical (CSS) pixels of the recorder window that takes mouse input.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct InputRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Makes only `rects` of the window receive mouse input; the rest is click-through.
#[tauri::command]
pub fn set_input_region(app: AppHandle, window: WebviewWindow, rects: Vec<InputRect>) {
    #[cfg(target_os = "linux")]
    {
        let _ = &app;
        set_input_shape(&window, rects);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let state = app.state::<RecorderState>();
        let first = {
            let mut interactive = state.interactive.lock().expect("input lock");
            let first = interactive.is_empty();
            *interactive = rects;
            first
        };
        if first {
            poll_click_through(window, Arc::clone(&state.interactive));
        }
    }
}

/// Linux (X11 and Wayland): GTK input shape of the toplevel window.
#[cfg(target_os = "linux")]
fn set_input_shape(window: &WebviewWindow, rects: Vec<InputRect>) {
    use gtk::cairo::{RectangleInt, Region};
    use gtk::prelude::*;

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(gtk_window) = target.gtk_window() else { return };
        let Some(gdk_window) = gtk_window.window() else { return };
        let rects: Vec<RectangleInt> = rects
            .iter()
            .map(|r| {
                RectangleInt::new(r.x.floor() as i32, r.y.floor() as i32, r.w.ceil() as i32 + 1, r.h.ceil() as i32 + 1)
            })
            .collect();
        let region = Region::create_rectangles(&rects);
        gdk_window.input_shape_combine_region(&region, 0, 0);
    });
}

/// Windows/macOS: there is no per-region click-through, so follow the cursor and let the whole
/// window ignore the mouse unless the cursor is over an interactive rect.
#[cfg(not(target_os = "linux"))]
fn poll_click_through(window: WebviewWindow, interactive: Arc<Mutex<Vec<InputRect>>>) {
    std::thread::spawn(move || {
        let mut ignoring = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(40));
            let (Ok(cursor), Ok(origin), Ok(scale)) =
                (window.cursor_position(), window.inner_position(), window.scale_factor())
            else {
                break; // window gone
            };
            let x = (cursor.x - f64::from(origin.x)) / scale;
            let y = (cursor.y - f64::from(origin.y)) / scale;
            let inside = interactive
                .lock()
                .expect("input lock")
                .iter()
                .any(|r| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h);
            if ignoring != Some(!inside) {
                if window.set_ignore_cursor_events(!inside).is_err() {
                    break;
                }
                ignoring = Some(!inside);
            }
        }
    });
}

// ---- locating the window on Wayland --------------------------------------------------------------

/// Colors of the calibration marker the recorder page draws in its top-left corner, as blocks of
/// `MARKER_BLOCK` logical pixels side by side. Keep in sync with `src/routes/recorder`.
const MARKER: [[u8; 3]; 4] = [[255, 0, 255], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
const MARKER_BLOCK: f64 = 6.0;

fn close_to(pixel: &[u8], color: [u8; 3]) -> bool {
    pixel.iter().zip(color).map(|(&a, b)| a.abs_diff(b) as u32).sum::<u32>() <= 60
}

/// Finds the calibration marker in a screenshot and returns its top-left corner in pixels.
/// `scale` is the monitor's scale factor (screen pixels per logical pixel).
pub fn find_marker(image: &RgbaImage, scale: f64) -> Option<(i32, i32)> {
    let block = MARKER_BLOCK * scale;
    let (w, h) = (image.width() as i64, image.height() as i64);
    let at = |x: f64, y: f64| -> Option<&[u8]> {
        let (x, y) = (x.round() as i64, y.round() as i64);
        (x >= 0 && y >= 0 && x < w && y < h).then(|| &image.get_pixel(x as u32, y as u32).0[..3])
    };
    for y in 0..h {
        for x in 0..w {
            if !close_to(&image.get_pixel(x as u32, y as u32).0[..3], MARKER[0]) {
                continue;
            }
            // Check the center of every block of the pattern.
            let matches = MARKER.iter().enumerate().all(|(i, &color)| {
                at(x as f64 + block * (i as f64 + 0.5), y as f64 + block / 2.0).is_some_and(|p| close_to(p, color))
            });
            if matches {
                return Some((x as i32, y as i32));
            }
        }
    }
    None
}

/// Wayland: where is the (maximized) recorder window? Takes a screenshot through the portal and
/// looks for the marker the page shows at its top-left corner. Returns screen pixels.
#[tauri::command]
pub async fn locate_marker(scale: f64) -> Result<Option<(i32, i32)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let image = framenab_capture::backend().screenshot(&CaptureTarget::AllMonitors).map_err(|e| e.to_string())?;
        let found = find_marker(&image, scale.max(0.5));
        tracing::debug!(?found, scale, "recorder window marker");
        Ok(found)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw_marker(image: &mut RgbaImage, x: u32, y: u32, scale: f64) {
        let block = (MARKER_BLOCK * scale).round() as u32;
        for (i, color) in MARKER.iter().enumerate() {
            for dy in 0..block {
                for dx in 0..block {
                    let [r, g, b] = *color;
                    image.put_pixel(x + i as u32 * block + dx, y + dy, image::Rgba([r, g, b, 255]));
                }
            }
        }
    }

    #[test]
    fn finds_the_marker_and_ignores_decoys() {
        for scale in [1.0, 1.25, 2.0] {
            let mut image = RgbaImage::from_pixel(400, 300, image::Rgba([30, 30, 40, 255]));
            // A lone magenta block (an icon, say) must not match.
            for dy in 0..10 {
                for dx in 0..10 {
                    image.put_pixel(20 + dx, 10 + dy, image::Rgba([255, 0, 255, 255]));
                }
            }
            draw_marker(&mut image, 57, 33, scale);
            assert_eq!(find_marker(&image, scale), Some((57, 33)), "scale {scale}");
        }
    }

    #[test]
    fn no_marker_no_match() {
        let image = RgbaImage::from_pixel(100, 100, image::Rgba([255, 0, 255, 255]));
        assert_eq!(find_marker(&image, 1.0), None);
    }
}
