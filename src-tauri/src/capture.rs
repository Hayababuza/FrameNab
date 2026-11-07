//! Screenshot flow: capture the screen → show the overlay with the frozen image → the user
//! selects and annotates an area → copy, save or pin it.
//!
//! The frozen image is kept in [`CaptureState`] and served to the overlay as raw RGBA through
//! the `framenab://` protocol, so no pixel data travels through JSON IPC.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use framenab_capture::{CaptureError, CaptureTarget};
use framenab_core::Annotation;
use framenab_export::still::{self, StillFormat};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeContext};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;

use crate::{clipboard, i18n::t, pin, settings, windows};

#[derive(Default)]
pub struct CaptureState {
    current: Mutex<Option<Capture>>,
    next_id: AtomicU64,
    /// A capture is in progress or the overlay is open.
    busy: AtomicBool,
    /// The main window was hidden for the capture and should come back afterwards.
    restore_main: AtomicBool,
}

struct Capture {
    id: u64,
    image: Arc<RgbaImage>,
    /// Already cropped to the overlay's monitor.
    fitted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    id: u64,
    width: u32,
    height: u32,
}

/// Selection in image pixels.
#[derive(Debug, Deserialize)]
pub struct Selection {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Starts a screenshot. `hide_main` hides the main window first so it is not in the picture.
pub fn start(app: &AppHandle, hide_main: bool) {
    let state = app.state::<CaptureState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        tracing::debug!("screenshot already in progress");
        windows::focus_overlay(app);
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let main_visible = windows::main_visible(&app);
        if hide_main && main_visible {
            windows::hide_main(&app);
            app.state::<CaptureState>().restore_main.store(true, Ordering::SeqCst);
        }

        let delay = hide_main && main_visible;
        let result = tauri::async_runtime::spawn_blocking(move || {
            if delay {
                // Give the compositor time to actually remove the window from the screen.
                std::thread::sleep(Duration::from_millis(300));
            }
            framenab_capture::backend().screenshot(&CaptureTarget::AllMonitors)
        })
        .await;

        let state = app.state::<CaptureState>();
        match result {
            Ok(Ok(image)) => {
                let id = state.next_id.fetch_add(1, Ordering::SeqCst) + 1;
                tracing::info!(id, width = image.width(), height = image.height(), "screenshot captured");
                *state.current.lock().expect("capture lock") =
                    Some(Capture { id, image: Arc::new(image), fitted: false });
                if let Err(err) = windows::open_overlay(&app) {
                    tracing::error!(%err, "failed to open overlay");
                    finish(&app);
                }
            }
            Ok(Err(CaptureError::PermissionDenied)) => {
                tracing::info!("screenshot cancelled by the user");
                finish(&app);
            }
            Ok(Err(err)) => {
                tracing::error!(%err, "screenshot failed");
                finish(&app);
                show_error(&app, &err.to_string());
            }
            Err(err) => {
                tracing::error!(%err, "screenshot task failed");
                finish(&app);
            }
        }
    });
}

/// Cleans up after the overlay closed (or the capture failed).
pub fn finish(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    state.current.lock().expect("capture lock").take();
    state.busy.store(false, Ordering::SeqCst);
    if state.restore_main.swap(false, Ordering::SeqCst) {
        windows::show_main(app);
    }
}

fn show_error(app: &AppHandle, detail: &str) {
    app.dialog()
        .message(format!("{}\n\n{detail}", t("capture.failed")))
        .title("FrameNab")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

/// Crops a screenshot of the whole virtual desktop to the monitor the overlay is shown on.
fn fit_to_monitor(app: &AppHandle, image: &RgbaImage) -> Option<RgbaImage> {
    let overlay = windows::overlay(app)?;
    let monitor = overlay.current_monitor().ok().flatten()?;
    let monitors = app.available_monitors().ok()?;
    if monitors.len() < 2 {
        return None;
    }
    let min_x = monitors.iter().map(|m| m.position().x).min()?;
    let min_y = monitors.iter().map(|m| m.position().y).min()?;
    let max_x = monitors.iter().map(|m| m.position().x + m.size().width as i32).max()?;
    let max_y = monitors.iter().map(|m| m.position().y + m.size().height as i32).max()?;
    if (image.width(), image.height()) != ((max_x - min_x) as u32, (max_y - min_y) as u32) {
        tracing::warn!("screenshot does not match the monitor layout; showing it unscaled");
        return None;
    }
    let pos = monitor.position();
    let size = monitor.size();
    Some(framenab_capture::crop(image, pos.x - min_x, pos.y - min_y, size.width, size.height))
}

#[tauri::command]
pub fn capture_info(app: AppHandle) -> Option<CaptureInfo> {
    let state = app.state::<CaptureState>();
    let mut guard = state.current.lock().expect("capture lock");
    let capture = guard.as_mut()?;
    if !capture.fitted {
        capture.fitted = true;
        if let Some(fitted) = fit_to_monitor(&app, &capture.image) {
            capture.image = Arc::new(fitted);
        }
    }
    Some(CaptureInfo { id: capture.id, width: capture.image.width(), height: capture.image.height() })
}

/// Crops the current capture to `selection` and renders the annotations into it.
async fn render_selection(
    app: &AppHandle,
    selection: Selection,
    annotations: Vec<Annotation>,
) -> Result<RgbaImage, String> {
    let image = {
        let state = app.state::<CaptureState>();
        let guard = state.current.lock().expect("capture lock");
        let capture = guard.as_ref().ok_or("no active screenshot")?;
        Arc::clone(&capture.image)
    };
    let Selection { x, y, width, height } = selection;
    tauri::async_runtime::spawn_blocking(move || {
        let mut cropped = framenab_capture::crop(&image, x as i32, y as i32, width.max(1), height.max(1));
        // Annotations are in screen-image coordinates; move them into the cropped area.
        let (dx, dy) = (-(x as f32), -(y as f32));
        let moved: Vec<Annotation> =
            annotations.into_iter().map(|a| Annotation { shape: a.shape.translated(dx, dy), ..a }).collect();
        framenab_render::render_annotations(&mut cropped, &moved).map(|()| cropped)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn copy_selection(app: AppHandle, selection: Selection, annotations: Vec<Annotation>) -> Result<(), String> {
    let result = render_selection(&app, selection, annotations).await?;
    tracing::info!(width = result.width(), height = result.height(), "copying selection to clipboard");
    clipboard::copy_image(&app, result).await?;
    windows::close_overlay(&app);
    Ok(())
}

/// Saves the selection. With `ask`, a file dialog picks the location; otherwise it goes to the
/// configured folder with the configured name template and format. Returns the path, or `None`
/// if the dialog was cancelled.
#[tauri::command]
pub async fn save_selection(
    app: AppHandle,
    selection: Selection,
    annotations: Vec<Annotation>,
    ask: bool,
) -> Result<Option<String>, String> {
    let result = render_selection(&app, selection, annotations).await?;
    let config = settings::save_settings(&app);
    let name = still::file_name_now(&config.template);

    let (path, format) = if ask {
        let Some(path) = ask_save_path(&app, &config.directory, &name, config.format).await? else {
            return Ok(None);
        };
        let format =
            path.extension().and_then(|e| e.to_str()).and_then(StillFormat::from_extension).unwrap_or(config.format);
        let path = if path.extension().is_none() { path.with_extension(format.extension()) } else { path };
        (path, format)
    } else {
        (still::unique_path(&config.directory, &name, config.format.extension()), config.format)
    };

    let target = path.clone();
    tauri::async_runtime::spawn_blocking(move || still::save_still(&result, &target, format))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    tracing::info!(path = %path.display(), "screenshot saved");
    windows::close_overlay(&app);

    let shown = path.display().to_string();
    if let Err(err) = app.notification().builder().title("FrameNab").body(format!("{}\n{shown}", t("save.done"))).show()
    {
        tracing::warn!(%err, "could not show notification");
    }
    Ok(Some(shown))
}

/// "Save as" dialog. The overlay is fullscreen and always on top, so it steps back while the
/// dialog is open.
async fn ask_save_path(
    app: &AppHandle,
    directory: &std::path::Path,
    name: &str,
    format: StillFormat,
) -> Result<Option<std::path::PathBuf>, String> {
    let overlay = windows::overlay(app);
    if let Some(w) = &overlay {
        let _ = w.set_always_on_top(false);
    }
    let _ = std::fs::create_dir_all(directory);
    let mut dialog = app
        .dialog()
        .file()
        .set_title(t("save.title"))
        .set_directory(directory)
        .set_file_name(format!("{name}.{}", format.extension()))
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .add_filter("WebP", &["webp"]);
    if let Some(w) = &overlay {
        dialog = dialog.set_parent(w);
    }
    let picked =
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await.map_err(|e| e.to_string())?;
    if let Some(w) = &overlay {
        let _ = w.set_always_on_top(true);
        let _ = w.set_focus();
    }
    picked.map(|p| p.into_path().map_err(|e| e.to_string())).transpose()
}

/// Pins the selection as a floating window. `scale` = image pixels per CSS pixel in the overlay.
#[tauri::command]
pub async fn pin_selection(
    app: AppHandle,
    selection: Selection,
    annotations: Vec<Annotation>,
    scale: f64,
) -> Result<(), String> {
    let result = render_selection(&app, selection, annotations).await?;
    windows::close_overlay(&app);
    pin::open(&app, result, scale.max(0.1)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_capture(app: AppHandle) {
    windows::close_overlay(&app);
}

/// Serves raw RGBA pixels: `framenab://localhost/<id>` for the current capture and
/// `framenab://localhost/pin/<id>` for pinned screenshots.
pub fn protocol<R: Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let respond = |status: StatusCode, body: Vec<u8>| {
        Response::builder()
            .status(status)
            .header("Content-Type", "application/octet-stream")
            .header("Access-Control-Allow-Origin", "*")
            .body(Cow::Owned(body))
            .expect("valid response")
    };
    let path = request.uri().path().trim_matches('/');
    let app = ctx.app_handle();
    if let Some(pin_id) = path.strip_prefix("pin/").and_then(|id| id.parse().ok()) {
        return match app.state::<pin::PinState>().image(pin_id) {
            Some(image) => respond(StatusCode::OK, image.as_raw().clone()),
            None => respond(StatusCode::NOT_FOUND, Vec::new()),
        };
    }
    let id: Option<u64> = path.parse().ok();
    let state = app.state::<CaptureState>();
    let guard = state.current.lock().expect("capture lock");
    match guard.as_ref() {
        Some(capture) if Some(capture.id) == id => respond(StatusCode::OK, capture.image.as_raw().clone()),
        _ => respond(StatusCode::NOT_FOUND, Vec::new()),
    }
}
