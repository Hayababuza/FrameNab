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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use framenab_capture::{CaptureTarget, Crop, FrameStream, Recording, StreamOptions, record};
use framenab_core::recorder::ProjectRecorder;
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
    /// The main window was hidden for the capture; show it again when the recorder closes.
    restore_main: AtomicBool,
    /// Click-through: only these rects (logical pixels) take mouse input (non-Linux).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    interactive: Arc<Mutex<Vec<InputRect>>>,
}

impl RecorderState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("recorder lock")
    }
}

/// Opens the recorder window for `region` (or a window chosen later, if `None`).
pub fn open(app: &AppHandle, region: Option<Region>, image_width: u32, image_height: u32, restore_main: bool) {
    let state = app.state::<RecorderState>();
    {
        let mut inner = state.lock();
        *inner = Inner::default();
        inner.setup = Some(Setup { region, image_width, image_height });
    }
    if restore_main {
        state.restore_main.store(true, Ordering::SeqCst);
    }
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
    if state.restore_main.swap(false, Ordering::SeqCst) {
        windows::show_main(app);
    }
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
fn recordings_dir(app: &AppHandle) -> PathBuf {
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
