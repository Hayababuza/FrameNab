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

/// Area to record, in pixels of the screenshot the overlay showed (= the monitor's pixels).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
}

#[derive(Default)]
struct Inner {
    setup: Option<Setup>,
    stream: Option<Box<dyn FrameStream>>,
    recording: Option<Recording<ProjectRecorder>>,
    project_dir: Option<PathBuf>,
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

/// Opens the recorder window for `region` (or a window chosen later, if `None`). `background` is
/// the frozen screen image of the area selection, if there was one.
pub fn open(
    app: &AppHandle,
    region: Option<Region>,
    (image_width, image_height): (u32, u32),
    background: Option<Arc<RgbaImage>>,
    restore: Option<String>,
) {
    let state = app.state::<RecorderState>();
    // Native Wayland windows cannot be transparent in fullscreen nor stay on top.
    let compact = crate::display::native_wayland();
    let background = background.filter(|_| compact);
    {
        let mut inner = state.lock();
        *inner = Inner::default();
        inner.setup = Some(Setup { region, image_width, image_height, background: background.is_some(), compact });
    }
    *state.background.lock().expect("background lock") = background;
    state.restore.set(restore);
    if let Err(err) = windows::open_recorder(app) {
        tracing::error!(%err, "failed to open the recorder window");
    }
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
    if let Some(recording) = recording {
        // Closed while recording: keep what was recorded.
        let (sink, end, _) = recording.stop();
        if let Err(err) = sink.finish(end) {
            tracing::warn!(%err, "could not finish the recording");
        } else if let Some(dir) = dir {
            tracing::info!(path = %dir.display(), "recording kept after the recorder was closed");
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
pub async fn recorder_prepare(app: AppHandle, cursor: bool) -> Result<(), String> {
    let setup = app.state::<RecorderState>().lock().setup.clone().ok_or("recorder is not set up")?;
    let store = app.store("settings.json").map_err(|e| e.to_string())?;
    let restore_token = store.get(TOKEN_KEY).and_then(|v| v.as_str().map(str::to_owned));
    let target = match setup.region {
        Some(r) => CaptureTarget::Region { x: r.x, y: r.y, width: r.w, height: r.h },
        None => CaptureTarget::Window(0),
    };

    let stream = tauri::async_runtime::spawn_blocking(move || {
        framenab_capture::backend().open_stream(&target, &StreamOptions { cursor, restore_token })
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

/// Starts sampling the prepared stream into a new project folder.
#[tauri::command]
pub fn recorder_start(app: AppHandle, fps: u32) -> Result<(), String> {
    let state = app.state::<RecorderState>();
    let mut inner = state.lock();
    let stream = inner.stream.take().ok_or("the stream is not prepared")?;
    let crop = inner.setup.as_ref().and_then(|s| s.region).map(|r| {
        let (ox, oy) = stream.origin();
        Crop { x: r.x - ox, y: r.y - oy, width: r.w, height: r.h }
    });
    let dir = recordings_dir(&app)
        .join(format!("{}.framenab", framenab_export::still::file_name_now(framenab_export::still::DEFAULT_TEMPLATE)));
    let sink = ProjectRecorder::create(&dir).map_err(|e| e.to_string())?;
    inner.recording = Some(record(stream, crop, fps, sink));
    inner.project_dir = Some(dir.clone());
    tracing::info!(path = %dir.display(), fps, ?crop, "recording started");
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
}

#[tauri::command]
pub async fn recorder_stop(app: AppHandle) -> Result<Summary, String> {
    let (recording, dir) = {
        let state = app.state::<RecorderState>();
        let mut inner = state.lock();
        (inner.recording.take().ok_or("not recording")?, inner.project_dir.clone().ok_or("no project folder")?)
    };
    let (project, stats) = tauri::async_runtime::spawn_blocking(move || {
        let (sink, end, _) = recording.stop();
        sink.finish(end)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    tracing::info!(path = %dir.display(), frames = stats.frames, dropped = stats.dropped, "recording finished");
    // Continue in the editor; it takes over bringing the main window back.
    let restore = app.state::<RecorderState>().restore.take();
    crate::editor::open(&app, dir.clone(), restore)?;
    Ok(Summary {
        path: dir.display().to_string(),
        frames: project.frames.len(),
        duration_ms: project.duration_ms(),
        width: project.width,
        height: project.height,
        dropped: stats.dropped,
    })
}

/// `<app data>/recordings`
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
