//! Editor for recordings (first version): a filmstrip of all frames, deleting frames with
//! undo/redo, playback, and saving the result as GIF/MP4/WebM into the save folder.
//!
//! The project folder (`*.framenab`) is the working copy: edits are written to its
//! `project.json` right away, frame files are only deleted when the editor closes.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::Duration;

use framenab_core::{FrameOp, FrameStore, History, ImageOp, Project, ops};
use framenab_export::ExportFormat;
use framenab_export::animation::{ExportControl, export_animation};
use image::codecs::jpeg::JpegEncoder;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_store::StoreExt;

use crate::{i18n::t, settings, windows};

pub const LABEL: &str = "editor";

struct Open {
    store: FrameStore,
    project: Project,
    history: History<Project>,
}

#[derive(Default)]
pub struct EditorState {
    open: Mutex<Option<Open>>,
    /// Filmstrip thumbnails (JPEG) by frame id; frames never change, so entries stay valid.
    thumbs: Mutex<std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>>,
    export: Mutex<Option<ExportControl>>,
    /// The main window was hidden for the capture; show it again when the editor closes.
    restore: windows::Restore,
}

impl EditorState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Open>> {
        self.open.lock().expect("editor lock")
    }
}

/// Opens the project folder `root` in the editor window.
pub fn open(app: &AppHandle, root: PathBuf, restore: Option<String>) -> Result<(), String> {
    let store = FrameStore::create(&root).map_err(|e| e.to_string())?;
    let project = store.load_project().map_err(|e| e.to_string())?;
    let state = app.state::<EditorState>();
    *state.lock() = Some(Open { store, project, history: History::new(200) });
    state.thumbs.lock().expect("thumb lock").clear();
    // Opening the editor shows it anyway; remember other windows (the main window).
    state.restore.set(restore.filter(|label| label != LABEL));

    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.emit("editor-reload", ());
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(LABEL.into()))
        .title("FrameNab – Editor")
        .inner_size(1100.0, 760.0)
        .min_inner_size(720.0, 480.0)
        .build()
        .map_err(|e| e.to_string())?;
    windows::fix_titlebar(&window);
    Ok(())
}

/// Called when the editor window is destroyed: drop unused frame files.
pub fn cleanup(app: &AppHandle) {
    let state = app.state::<EditorState>();
    if let Some(control) = state.export.lock().expect("export lock").as_ref() {
        control.cancel.store(true, Ordering::Relaxed);
    }
    if let Some(open) = state.lock().take()
        && let Err(err) = open.store.collect_garbage(&open.project)
    {
        tracing::warn!(%err, "could not remove unused frames");
    }
    state.restore.restore(app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorView {
    project: Project,
    path: String,
    can_undo: bool,
    can_redo: bool,
}

fn view(open: &Open) -> EditorView {
    EditorView {
        project: open.project.clone(),
        path: open.store.root().display().to_string(),
        can_undo: open.history.can_undo(),
        can_redo: open.history.can_redo(),
    }
}

/// Applies an edit, remembers the previous state for undo and saves `project.json`.
fn edit(app: &AppHandle, change: impl FnOnce(&mut Project) -> framenab_core::Result<()>) -> Result<EditorView, String> {
    let state = app.state::<EditorState>();
    let mut guard = state.lock();
    let open = guard.as_mut().ok_or("no project open")?;
    let before = open.project.clone();
    change(&mut open.project).map_err(|e| e.to_string())?;
    open.history.record(&before);
    open.store.save_project(&open.project).map_err(|e| e.to_string())?;
    Ok(view(open))
}

#[tauri::command]
pub fn editor_view(app: AppHandle) -> Option<EditorView> {
    app.state::<EditorState>().lock().as_ref().map(view)
}

/// Applies a frame operation (delete, duplicate, reverse, timing …). Runs off the main thread:
/// removing similar frames loads and compares every image.
#[tauri::command]
pub async fn editor_frames(app: AppHandle, op: FrameOp) -> Result<EditorView, String> {
    if let FrameOp::Delete { frames } = &op {
        let len = app.state::<EditorState>().lock().as_ref().map_or(0, |o| o.project.frames.len());
        if frames.len() >= len {
            return Err("at least one frame has to stay".into());
        }
    }
    let (store, before) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        (open.store.clone(), open.project.clone())
    };
    let changed = tauri::async_runtime::spawn_blocking(move || {
        let mut project = before.clone();
        ops::apply(&mut project, &op, |frame| store.load_frame(frame)).map(|()| (before, project))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let (before, project) = changed;
    edit(&app, move |p| {
        // Only if nothing else changed the project meanwhile.
        if *p != before {
            return Err(framenab_core::CoreError::Busy);
        }
        *p = project;
        Ok(())
    })
}

/// Crops, resizes, rotates or flips every frame. Writes new frame files in the background;
/// progress goes to the editor as `image-progress` events (`[done, total]`).
#[tauri::command]
pub async fn editor_image(app: AppHandle, op: ImageOp) -> Result<EditorView, String> {
    let (store, before) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        (open.store.clone(), open.project.clone())
    };
    let total = before.frames.iter().map(|f| &f.file).collect::<std::collections::HashSet<_>>().len();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ticker = {
        let (app, done) = (app.clone(), std::sync::Arc::clone(&done));
        tauri::async_runtime::spawn(async move {
            loop {
                let n = done.load(Ordering::Relaxed).min(total);
                let _ = app.emit_to(LABEL, "image-progress", (n, total));
                if n >= total {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        })
    };
    let worker_done = std::sync::Arc::clone(&done);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut project = before.clone();
        framenab_core::image_ops::apply_image_op(&store, &mut project, &op, &worker_done).map(|()| (before, project))
    })
    .await
    .map_err(|e| e.to_string());
    done.store(total, Ordering::Relaxed);
    let _ = ticker.await;
    let (before, project) = result?.map_err(|e| e.to_string())?;
    edit(&app, move |p| {
        if *p != before {
            return Err(framenab_core::CoreError::Busy);
        }
        *p = project;
        Ok(())
    })
}

/// The input recorded with the open project (mouse, clicks, keys) for the effect preview.
#[tauri::command]
pub fn editor_input(app: AppHandle) -> Result<framenab_core::InputEvents, String> {
    let state = app.state::<EditorState>();
    let guard = state.lock();
    let open = guard.as_ref().ok_or("no project open")?;
    open.store.load_input().map(|input| input.transformed(&open.project.input_transform)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn editor_set_effects(app: AppHandle, effects: framenab_core::Effects) -> Result<EditorView, String> {
    edit(&app, |p| {
        p.effects = effects;
        Ok(())
    })
}

/// Replaces all annotations (the editor sends the whole list after each change).
#[tauri::command]
pub fn editor_set_annotations(
    app: AppHandle,
    annotations: Vec<framenab_core::Annotation>,
) -> Result<EditorView, String> {
    edit(&app, |p| {
        p.annotations = annotations;
        Ok(())
    })
}

#[tauri::command]
pub fn editor_undo(app: AppHandle, redo: bool) -> Result<EditorView, String> {
    let state = app.state::<EditorState>();
    let mut guard = state.lock();
    let open = guard.as_mut().ok_or("no project open")?;
    let restored = if redo { open.history.redo(&open.project) } else { open.history.undo(&open.project) };
    if let Some(project) = restored {
        open.project = project;
        open.store.save_project(&open.project).map_err(|e| e.to_string())?;
    }
    Ok(view(open))
}

/// Settings key for the format of saved recordings.
const FORMAT_KEY: &str = "recordingFormat";

/// Saves the project as an animation: into the save folder in the format from the settings, or
/// with `ask` wherever and in whatever format the user picks in a "Save as" dialog. Resolves to
/// the path, or `None` if the dialog was cancelled. Progress goes to the editor window as
/// `export-progress` events (`[done, total]`).
#[tauri::command]
pub async fn editor_export(app: AppHandle, ask: bool) -> Result<Option<String>, String> {
    let (store, project) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        (open.store.clone(), open.project.clone())
    };
    let format: ExportFormat = app
        .store("settings.json")
        .ok()
        .and_then(|s| s.get(FORMAT_KEY))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or(ExportFormat::Gif);
    let config = settings::save_settings(&app);
    let name = framenab_export::still::file_name_now(&config.template);
    let (path, format) = if ask {
        let Some(path) = ask_path(&app, &config.directory, &name, format).await? else { return Ok(None) };
        let picked = path.extension().and_then(|e| e.to_str()).and_then(format_for_extension);
        let format = picked.unwrap_or(format);
        let path = if picked.is_none() { path.with_extension(format.extension()) } else { path };
        (path, format)
    } else {
        (framenab_export::still::unique_path(&config.directory, &name, format.extension()), format)
    };

    let control = ExportControl::default();
    *app.state::<EditorState>().export.lock().expect("export lock") = Some(control.clone());
    let total = project.frames.len();

    // Report progress while the export runs.
    let ticker = {
        let (app, control) = (app.clone(), control.clone());
        tauri::async_runtime::spawn(async move {
            loop {
                let done = control.done.load(Ordering::Relaxed).min(total);
                let _ = app.emit_to(LABEL, "export-progress", (done, total));
                if done >= total || control.cancel.load(Ordering::Relaxed) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        })
    };

    let target = path.clone();
    let worker_control = control.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        export_animation(&store, &project, &target, format, &worker_control)
    })
    .await
    .map_err(|e| e.to_string())?;
    control.done.store(total, Ordering::Relaxed);
    let _ = ticker.await;
    app.state::<EditorState>().export.lock().expect("export lock").take();
    result.map_err(|e| e.to_string())?;

    let shown = path.display().to_string();
    tracing::info!(path = %shown, ?format, "recording saved");
    if let Err(err) =
        app.notification().builder().title("FrameNab").body(format!("{}\n{shown}", t("save.recordingDone"))).show()
    {
        tracing::warn!(%err, "could not show notification");
    }
    Ok(Some(shown))
}

fn format_for_extension(ext: &str) -> Option<ExportFormat> {
    match ext.to_ascii_lowercase().as_str() {
        "gif" => Some(ExportFormat::Gif),
        "mp4" | "m4v" => Some(ExportFormat::Mp4),
        "webm" => Some(ExportFormat::Webm),
        _ => None,
    }
}

/// "Save as" dialog for recordings; offers the formats that work on this system.
async fn ask_path(
    app: &AppHandle,
    directory: &std::path::Path,
    name: &str,
    format: ExportFormat,
) -> Result<Option<PathBuf>, String> {
    use tauri_plugin_dialog::DialogExt;

    let _ = std::fs::create_dir_all(directory);
    let mut dialog = app
        .dialog()
        .file()
        .set_title(t("save.recordingTitle"))
        .set_directory(directory)
        .set_file_name(format!("{name}.{}", format.extension()));
    // The default format first, so the dialog starts with it.
    let mut formats = recording_formats();
    formats.sort_by_key(|f| *f != format);
    for f in formats {
        let (label, extensions): (&str, &[&str]) = match f {
            ExportFormat::Gif => ("GIF", &["gif"]),
            ExportFormat::Mp4 => ("MP4 (H.264)", &["mp4"]),
            _ => ("WebM (VP9)", &["webm"]),
        };
        dialog = dialog.add_filter(label, extensions);
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        dialog = dialog.set_parent(&window);
    }
    let picked =
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await.map_err(|e| e.to_string())?;
    picked.map(|p| p.into_path().map_err(|e| e.to_string())).transpose()
}

#[tauri::command]
pub fn editor_cancel_export(app: AppHandle) {
    if let Some(control) = app.state::<EditorState>().export.lock().expect("export lock").as_ref() {
        control.cancel.store(true, Ordering::Relaxed);
    }
}

// ---- images for the UI ------------------------------------------------------------------------

/// Height of filmstrip thumbnails in pixels.
const THUMB_HEIGHT: u32 = 90;
/// Largest preview size in pixels.
const PREVIEW_MAX: (u32, u32) = (1600, 900);

/// Serves `thumb/<frame id>` and `preview/<frame id>` as JPEG for the editor.
pub fn serve<R: tauri::Runtime>(app: &AppHandle<R>, path: &str) -> Option<Vec<u8>> {
    let (kind, id) = path.split_once('/')?;
    if kind == "thumb"
        && let Some(cached) = app.state::<EditorState>().thumbs.lock().expect("thumb lock").get(id)
    {
        return Some(cached.to_vec());
    }
    let (store, frame) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref()?;
        let frame = open.project.frames.iter().find(|f| f.id == id)?.clone();
        (open.store.clone(), frame)
    };
    let image = store.load_frame(&frame).ok()?;
    let (w, h) = image.dimensions();
    let (tw, th) = match kind {
        "thumb" => ((w * THUMB_HEIGHT / h.max(1)).max(1), THUMB_HEIGHT),
        "preview" => {
            let scale = (f64::from(PREVIEW_MAX.0) / f64::from(w)).min(f64::from(PREVIEW_MAX.1) / f64::from(h)).min(1.0);
            ((f64::from(w) * scale).round().max(1.0) as u32, (f64::from(h) * scale).round().max(1.0) as u32)
        }
        _ => return None,
    };
    let scaled = if (tw, th) == (w, h) { image } else { image::imageops::thumbnail(&image, tw, th) };
    let rgb = image::DynamicImage::ImageRgba8(scaled).into_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, if kind == "thumb" { 75 } else { 88 }).encode_image(&rgb).ok()?;
    if kind == "thumb" {
        app.state::<EditorState>()
            .thumbs
            .lock()
            .expect("thumb lock")
            .insert(id.to_owned(), std::sync::Arc::new(out.clone()));
    }
    Some(out)
}

/// Format names offered for recordings, so the settings page can show what works here.
#[tauri::command]
pub fn recording_formats() -> Vec<ExportFormat> {
    let ffmpeg = framenab_export::find_ffmpeg().is_some();
    [ExportFormat::Gif, ExportFormat::Mp4, ExportFormat::Webm]
        .into_iter()
        .filter(|f| ffmpeg || !f.requires_ffmpeg())
        .collect()
}

/// "Open project" in the main window.
#[tauri::command]
pub fn open_project(app: AppHandle, path: PathBuf) -> Result<(), String> {
    open(&app, path, None)
}

/// Where recordings are kept, as the start folder for "Open project".
#[tauri::command]
pub fn recordings_directory(app: AppHandle) -> String {
    crate::recorder::recordings_dir(&app).display().to_string()
}
