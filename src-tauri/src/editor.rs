//! Editor for recordings (first version): a filmstrip of all frames, deleting frames with
//! undo/redo, playback, and saving the result as GIF/MP4/WebM into the save folder.
//!
//! The project folder (`*.framenab`) is the working copy: edits are written to its
//! `project.json` right away, frame files are only deleted when the editor closes.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::Duration;

use framenab_core::{FrameOp, FrameStore, History, ImageOp, Project, TransitionOp, ops};
use framenab_export::animation::{ExportControl, export_animation};
use framenab_export::{ExportFormat, ExportOptions};
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

/// Opens a project in the editor window: a project folder, its `project.json`, or a project
/// archive (`*.framenab.zip`, extracted into the recordings folder first).
pub fn open(app: &AppHandle, path: PathBuf, restore: Option<String>) -> Result<(), String> {
    let root = if path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        let dir = crate::recorder::recordings_dir(app);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        framenab_core::archive::import_zip(&path, &dir).map_err(|e| e.to_string())?
    } else if path.is_file() && path.file_name().is_some_and(|n| n == "project.json") {
        path.parent().map(PathBuf::from).ok_or("invalid path")?
    } else {
        path
    };
    let store = FrameStore::create(&root).map_err(|e| e.to_string())?;
    let (project, recovered) = store.recover_project().map_err(|e| e.to_string())?;
    if recovered {
        tracing::info!(path = %root.display(), frames = project.frames.len(), "recovered an interrupted recording");
    }
    let state = app.state::<EditorState>();
    *state.lock() = Some(Open { store, project, history: History::new(200) });
    state.thumbs.lock().expect("thumb lock").clear();
    // Opening the editor shows it anyway; remember other windows (the main window).
    state.restore.set(restore.filter(|label| label != LABEL));

    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.emit("editor-reload", ());
        let _ = w.show();
        windows::focus(&w);
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(LABEL.into()))
        .title("FrameNab – Editor")
        .inner_size(1100.0, 760.0)
        .min_inner_size(720.0, 480.0)
        .build()
        .map_err(|e| e.to_string())?;
    windows::fix_titlebar(&window);
    windows::focus(&window);
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

/// Runs `change` on a copy of the open project off the main thread (it may load and write
/// frame images), then takes the result over as one undoable edit.
async fn background_edit(
    app: &AppHandle,
    change: impl FnOnce(&FrameStore, &mut Project) -> framenab_core::Result<()> + Send + 'static,
) -> Result<EditorView, String> {
    let (store, before) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        (open.store.clone(), open.project.clone())
    };
    let (before, project) = tauri::async_runtime::spawn_blocking(move || {
        let mut project = before.clone();
        change(&store, &mut project).map(|()| (before, project))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    edit(app, move |p| {
        // Only if nothing else changed the project meanwhile.
        if *p != before {
            return Err(framenab_core::CoreError::Busy);
        }
        *p = project;
        Ok(())
    })
}

/// Applies a frame operation (delete, duplicate, reverse, timing …). Removing similar frames
/// loads and compares every image.
#[tauri::command]
pub async fn editor_frames(app: AppHandle, op: FrameOp) -> Result<EditorView, String> {
    if let FrameOp::Delete { frames } = &op {
        let len = app.state::<EditorState>().lock().as_ref().map_or(0, |o| o.project.frames.len());
        if frames.len() >= len {
            return Err("at least one frame has to stay".into());
        }
    }
    background_edit(&app, move |store, project| ops::apply(project, &op, |frame| store.load_frame(frame))).await
}

/// Crops, resizes, rotates or flips every frame. Progress goes to the editor as
/// `image-progress` events (`[done, total]`).
#[tauri::command]
pub async fn editor_image(app: AppHandle, op: ImageOp) -> Result<EditorView, String> {
    let total = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        open.project.frames.iter().map(|f| &f.file).collect::<std::collections::HashSet<_>>().len()
    };
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
    let result = background_edit(&app, move |store, project| {
        framenab_core::image_ops::apply_image_op(store, project, &op, &worker_done)
    })
    .await;
    done.store(total, Ordering::Relaxed);
    let _ = ticker.await;
    result
}

/// Inserts transition frames (fade, slide) after a frame.
#[tauri::command]
pub async fn editor_transition(app: AppHandle, op: TransitionOp) -> Result<EditorView, String> {
    background_edit(&app, move |store, project| framenab_core::transitions::insert_transition(store, project, &op))
        .await
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

/// Sets the progress bar and watermark settings.
#[tauri::command]
pub fn editor_set_overlays(app: AppHandle, overlays: framenab_core::overlays::Overlays) -> Result<EditorView, String> {
    if let Some(wm) = &overlays.watermark
        && !framenab_core::store::is_asset_name(&wm.file)
    {
        return Err("invalid watermark file".into());
    }
    edit(&app, |p| {
        p.overlays = overlays;
        Ok(())
    })
}

/// Lets the user pick an image and makes it the watermark (copied into the project).
/// Resolves to `None` if the dialog was cancelled.
#[tauri::command]
pub async fn editor_pick_watermark(app: AppHandle) -> Result<Option<EditorView>, String> {
    use framenab_core::overlays::{Anchor, Watermark};
    use tauri_plugin_dialog::DialogExt;

    let store = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        guard.as_ref().ok_or("no project open")?.store.clone()
    };
    let mut dialog = app.dialog().file().add_filter("Image", &["png", "jpg", "jpeg", "webp", "gif", "bmp"]);
    if let Some(window) = app.get_webview_window(LABEL) {
        dialog = dialog.set_parent(&window);
    }
    let picked =
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file()).await.map_err(|e| e.to_string())?;
    let Some(source) = picked.map(|p| p.into_path().map_err(|e| e.to_string())).transpose()? else { return Ok(None) };
    let (file, width, height) = tauri::async_runtime::spawn_blocking(move || store.import_asset(&source, "watermark"))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    edit(&app, |p| {
        let previous = p.overlays.watermark.take();
        p.overlays.watermark = Some(Watermark {
            file,
            image_width: width,
            image_height: height,
            anchor: previous.as_ref().map_or(Anchor::BottomRight, |w| w.anchor),
            size: previous.as_ref().map_or(0.2, |w| w.size),
            opacity: previous.as_ref().map_or(0.8, |w| w.opacity),
        });
        Ok(())
    })
    .map(Some)
}

/// Serves `asset/<file>` (the watermark image) of the open project.
pub fn serve_asset(app: &AppHandle<impl tauri::Runtime>, file: &str) -> Option<Vec<u8>> {
    if !framenab_core::store::is_asset_name(file) {
        return None;
    }
    let root = app.state::<EditorState>().lock().as_ref()?.store.root().to_path_buf();
    std::fs::read(root.join(file)).ok()
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
/// Settings key for the export options (quality, lossy, loops, scale).
const OPTIONS_KEY: &str = "exportOptions";
/// Settings key: put a saved recording on the clipboard as a file (default on).
const COPY_KEY: &str = "copyRecording";

/// The format and options for saving without asking, from the settings.
pub fn saved_choice(app: &AppHandle) -> (ExportFormat, ExportOptions) {
    let saved = |key: &str| app.store("settings.json").ok().and_then(|s| s.get(key));
    let format = saved(FORMAT_KEY).and_then(|v| serde_json::from_value(v).ok()).unwrap_or(ExportFormat::Gif);
    let options = saved(OPTIONS_KEY).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    (format, options)
}

/// Whether saved recordings go to the clipboard (setting, default on).
pub fn copy_setting(app: &AppHandle) -> bool {
    app.store("settings.json").ok().and_then(|s| s.get(COPY_KEY)).and_then(|v| v.as_bool()).unwrap_or(true)
}

/// After a recording was saved: copies the file to the clipboard (with `copy`) and tells the user
/// where it is.
pub async fn announce_saved(app: &AppHandle, path: &std::path::Path, copy: bool) {
    let copied = copy
        && match crate::clipboard::copy_file(app, path.to_owned()).await {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(%err, "could not copy the recording to the clipboard");
                false
            }
        };
    let shown = path.display().to_string();
    tracing::info!(path = %shown, copied, "recording saved");
    let text = if copied { t("save.recordingCopied") } else { t("save.recordingDone") };
    if let Err(err) = app.notification().builder().title("FrameNab").body(format!("{text}\n{shown}")).show() {
        tracing::warn!(%err, "could not show notification");
    }
}

/// Estimated file size in bytes of saving the open project in `format` with `options`.
#[tauri::command]
pub async fn editor_estimate(app: AppHandle, format: ExportFormat, options: ExportOptions) -> Result<u64, String> {
    let (store, project) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        (open.store.clone(), open.project.clone())
    };
    tauri::async_runtime::spawn_blocking(move || {
        framenab_export::animation::estimate_size(&store, &project, format, &options)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Saves the project as an animation: into the save folder in the format from the settings, or
/// with `ask` wherever and in whatever format the user picks in a "Save as" dialog. Resolves to
/// the path, or `None` if the dialog was cancelled. Progress goes to the editor window as
/// `export-progress` events (`[done, total]`).
#[tauri::command]
pub async fn editor_export(
    app: AppHandle,
    ask: bool,
    format: Option<ExportFormat>,
    options: Option<ExportOptions>,
) -> Result<Option<String>, String> {
    let (saved_format, saved_options) = saved_choice(&app);
    let format = format.unwrap_or(saved_format);
    let options = options.unwrap_or(saved_options);
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
    export_open_project(&app, &path, format, options).await?;
    announce_saved(&app, &path, copy_setting(&app)).await;
    Ok(Some(path.display().to_string()))
}

/// Saves the open project as a GIF and puts the file on the clipboard, to paste it right away.
/// The file goes to the app's cache (`clipboard/`, files older than a day are removed), not the
/// save folder. Progress as with [`editor_export`]. Resolves to the file.
#[tauri::command]
pub async fn editor_copy(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("clipboard");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    remove_older_than(&dir, Duration::from_secs(24 * 3600));
    let (_, options) = saved_choice(&app);
    let config = settings::save_settings(&app);
    let name = framenab_export::still::file_name_now(&config.template);
    let path = framenab_export::still::unique_path(&dir, &name, ExportFormat::Gif.extension());
    export_open_project(&app, &path, ExportFormat::Gif, options).await?;
    crate::clipboard::copy_file(&app, path.clone()).await?;
    tracing::info!(path = %path.display(), "recording copied to the clipboard");
    Ok(path.display().to_string())
}

/// Removes the files in `dir` that were not changed for `age`.
fn remove_older_than(dir: &std::path::Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|e| e > age);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Exports the open project to `path`, reporting progress to the editor (`export-progress`,
/// `[done, total]`); can be cancelled with `editor_cancel_export`.
async fn export_open_project(
    app: &AppHandle,
    path: &std::path::Path,
    format: ExportFormat,
    options: ExportOptions,
) -> Result<(), String> {
    let (store, project) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        (open.store.clone(), open.project.clone())
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

    let target = path.to_owned();
    let worker_control = control.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        export_animation(&store, &project, &target, format, &options, &worker_control)
    })
    .await
    .map_err(|e| e.to_string())?;
    control.done.store(total, Ordering::Relaxed);
    let _ = ticker.await;
    app.state::<EditorState>().export.lock().expect("export lock").take();
    result.map_err(|e| e.to_string())
}

fn format_for_extension(ext: &str) -> Option<ExportFormat> {
    match ext.to_ascii_lowercase().as_str() {
        "gif" => Some(ExportFormat::Gif),
        "png" | "apng" => Some(ExportFormat::Apng),
        "webp" => Some(ExportFormat::Webp),
        "zip" => Some(ExportFormat::PngSequence),
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
            ExportFormat::Apng => ("APNG", &["png", "apng"]),
            ExportFormat::Webp => ("WebP", &["webp"]),
            ExportFormat::PngSequence => ("PNG (ZIP)", &["zip"]),
            ExportFormat::Mp4 => ("MP4 (H.264)", &["mp4"]),
            ExportFormat::Webm => ("WebM (VP9)", &["webm"]),
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

/// Exports the open project as a `*.framenab.zip` archive (to pass it on or keep it).
/// Resolves to the path, or `None` if the dialog was cancelled.
#[tauri::command]
pub async fn editor_export_project(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (store, project) = {
        let state = app.state::<EditorState>();
        let guard = state.lock();
        let open = guard.as_ref().ok_or("no project open")?;
        (open.store.clone(), open.project.clone())
    };
    let config = settings::save_settings(&app);
    let name = store
        .root()
        .file_stem()
        .and_then(|s| s.to_str())
        .map_or_else(|| framenab_export::still::file_name_now(&config.template), String::from);
    let _ = std::fs::create_dir_all(&config.directory);
    let mut dialog = app
        .dialog()
        .file()
        .set_title(t("save.projectTitle"))
        .set_directory(&config.directory)
        .set_file_name(format!("{name}.{}", framenab_core::archive::EXTENSION))
        .add_filter(t("save.projectFilter"), &["zip"]);
    if let Some(window) = app.get_webview_window(LABEL) {
        dialog = dialog.set_parent(&window);
    }
    let picked =
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await.map_err(|e| e.to_string())?;
    let Some(path) = picked.map(|p| p.into_path().map_err(|e| e.to_string())).transpose()? else { return Ok(None) };
    let path = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        path
    } else {
        path.with_extension(framenab_core::archive::EXTENSION)
    };
    let target = path.clone();
    tauri::async_runtime::spawn_blocking(move || framenab_core::archive::export_zip(&store, &project, &target))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    tracing::info!(path = %path.display(), "project exported");
    Ok(Some(path.display().to_string()))
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
    framenab_export::available_formats()
}

/// "Open project" in the main window.
#[tauri::command]
// `async`: opens the editor window (a synchronous command would deadlock on Windows, wry#583).
pub async fn open_project(app: AppHandle, path: PathBuf) -> Result<(), String> {
    open(&app, path, None)
}

/// A recording in the recordings folder, for the list on the start page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    path: String,
    /// Last change, in ms since the Unix epoch.
    modified_ms: u64,
    frames: usize,
    duration_ms: u64,
    width: u32,
    height: u32,
    /// The recording did not finish (crash); opening it recovers what was written.
    interrupted: bool,
}

/// The `limit` most recently changed recordings.
#[tauri::command]
pub fn recent_projects(app: AppHandle, limit: usize) -> Vec<RecentProject> {
    let mut folders = crate::cleanup::recording_folders(&crate::recorder::recordings_dir(&app));
    folders.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    folders
        .into_iter()
        .take(limit)
        .map(|(modified, path)| {
            let project = FrameStore::create(&path).ok().and_then(|s| s.load_project().ok());
            let frames_on_disk = || std::fs::read_dir(path.join("frames")).map_or(0, Iterator::count);
            RecentProject {
                modified_ms: modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0)),
                frames: project.as_ref().map_or_else(frames_on_disk, |p| p.frames.len()),
                duration_ms: project.as_ref().map_or(0, Project::duration_ms),
                width: project.as_ref().map_or(0, |p| p.width),
                height: project.as_ref().map_or(0, |p| p.height),
                interrupted: project.as_ref().is_none_or(|p| p.recording),
                path: path.display().to_string(),
            }
        })
        .collect()
}

/// Folder of the project open in the editor.
pub fn open_root(app: &AppHandle) -> Option<PathBuf> {
    app.state::<EditorState>().lock().as_ref().map(|o| o.store.root().to_path_buf())
}

/// Deletes a recording folder. Only folders inside the recordings folder can be deleted.
#[tauri::command]
pub fn delete_project(app: AppHandle, path: PathBuf) -> Result<(), String> {
    let dir = crate::recorder::recordings_dir(&app).canonicalize().map_err(|e| e.to_string())?;
    let target = path.canonicalize().map_err(|e| e.to_string())?;
    if target.parent() != Some(dir.as_path()) || target.extension().is_none_or(|e| e != "framenab") {
        return Err("not a recording".into());
    }
    let state = app.state::<EditorState>();
    if state.lock().as_ref().is_some_and(|o| o.store.root().canonicalize().ok().as_ref() == Some(&target)) {
        return Err("the recording is open in the editor".into());
    }
    std::fs::remove_dir_all(&target).map_err(|e| e.to_string())
}

/// Where recordings are kept, as the start folder for "Open project".
#[tauri::command]
pub fn recordings_directory(app: AppHandle) -> String {
    crate::recorder::recordings_dir(&app).display().to_string()
}
