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
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;

use crate::{clipboard, i18n::t, pin, recorder, settings, windows};

/// What the overlay is for: a screenshot, or choosing the area of a recording.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureMode {
    #[default]
    Screenshot,
    Record,
}

#[derive(Default)]
pub struct CaptureState {
    current: Mutex<Option<Capture>>,
    mode: Mutex<CaptureMode>,
    next_id: AtomicU64,
    /// A capture is in progress or the overlay is open.
    busy: AtomicBool,
    /// The main window was hidden for the capture and should come back afterwards.
    restore: windows::Restore,
}

struct Capture {
    id: u64,
    image: Arc<RgbaImage>,
    /// Other apps' windows in image pixels, topmost first (empty where unsupported).
    windows: Vec<WindowRect>,
    /// Already cropped to the overlay's monitor.
    fitted: bool,
}

/// A window's frame in image pixels, for snapping the selection to it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct WindowRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    id: u64,
    width: u32,
    height: u32,
    windows: Vec<WindowRect>,
    mode: CaptureMode,
    /// Whether the platform can record a single window chosen in a system dialog (Wayland).
    window_picker: bool,
}

/// Selection in image pixels.
#[derive(Debug, Deserialize)]
pub struct Selection {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Starts a screenshot (or the area selection of a recording). `hide` is the window the request
/// came from: it is hidden so it is not in the picture, and restored afterwards.
pub fn start(app: &AppHandle, hide: Option<String>, mode: CaptureMode) {
    let state = app.state::<CaptureState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        tracing::debug!("screenshot already in progress");
        windows::focus_overlay(app);
        return;
    }
    *state.mode.lock().expect("mode lock") = mode;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let hidden = hide.filter(|label| windows::hide_window(&app, label));
        let delay = hidden.is_some();
        app.state::<CaptureState>().restore.set(hidden);
        let app_for_access = app.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            if delay {
                // Give the compositor time to actually remove the window from the screen.
                std::thread::sleep(Duration::from_millis(300));
            }
            let backend = framenab_capture::backend();
            let image = match backend.screenshot(&CaptureTarget::AllMonitors) {
                Err(CaptureError::AccessRequired) => {
                    request_access(&app_for_access)?;
                    std::thread::sleep(Duration::from_millis(300));
                    backend.screenshot(&CaptureTarget::AllMonitors)?
                }
                other => other?,
            };
            // Window frames are in desktop coordinates; the image starts at the desktop's top-left.
            let (min_x, min_y, _, _) =
                framenab_capture::bounds(&backend.monitors().unwrap_or_default()).unwrap_or_default();
            let windows = backend
                .windows()
                .unwrap_or_else(|err| {
                    tracing::debug!(%err, "window list unavailable");
                    Vec::new()
                })
                .into_iter()
                .map(|w| WindowRect { x: w.x - min_x, y: w.y - min_y, w: w.width, h: w.height })
                .collect::<Vec<_>>();
            Ok::<_, CaptureError>((image, windows))
        })
        .await;

        let state = app.state::<CaptureState>();
        match result {
            Ok(Ok((image, windows))) => {
                let id = state.next_id.fetch_add(1, Ordering::SeqCst) + 1;
                tracing::info!(id, width = image.width(), height = image.height(), "screenshot captured");
                *state.current.lock().expect("capture lock") =
                    Some(Capture { id, image: Arc::new(image), windows, fitted: false });
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

/// The first screenshot of the installed app on GNOME: the system asks once whether FrameNab may
/// take screenshots, but only lets the focused app ask. Brings FrameNab to the front, explains
/// what comes next (the click on OK gives it the focus), asks through the portal while it is
/// focused, and hides it again. Afterwards the permission is stored and no dialog appears.
fn request_access(app: &AppHandle) -> Result<(), CaptureError> {
    use tauri_plugin_dialog::MessageDialogButtons;

    tracing::info!("screenshot permission missing; asking with FrameNab in front");
    windows::show_main(app);
    let mut dialog =
        app.dialog().message(t("capture.accessExplain")).title("FrameNab").kind(MessageDialogKind::Info).buttons(
            MessageDialogButtons::OkCancelCustom(t("capture.accessContinue").into(), t("capture.accessCancel").into()),
        );
    if let Some(main) = app.get_webview_window(windows::MAIN) {
        dialog = dialog.parent(&main);
    }
    if !dialog.blocking_show() {
        let _ = windows::hide_window(app, windows::MAIN);
        return Err(CaptureError::PermissionDenied);
    }
    // While FrameNab has the focus, the portal may show its access dialog.
    let granted = framenab_capture::backend().screenshot(&CaptureTarget::AllMonitors);
    let _ = windows::hide_window(app, windows::MAIN);
    match granted {
        Ok(_) => Ok(()),
        Err(CaptureError::AccessRequired) => Err(CaptureError::Backend(t("capture.accessDenied").into())),
        Err(other) => Err(other),
    }
}

/// Cleans up after the overlay closed (or the capture failed).
pub fn finish(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    state.current.lock().expect("capture lock").take();
    state.busy.store(false, Ordering::SeqCst);
    state.restore.restore(app);
}

fn show_error(app: &AppHandle, detail: &str) {
    app.dialog()
        .message(format!("{}\n\n{detail}", t("capture.failed")))
        .title("FrameNab")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

/// Crops a screenshot of the whole virtual desktop to the monitor the overlay is shown on.
/// Returns the cropped image and the offset of the crop within the original.
fn fit_to_monitor(app: &AppHandle, image: &RgbaImage) -> Option<(RgbaImage, i32, i32)> {
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
    let (ox, oy) = (pos.x - min_x, pos.y - min_y);
    Some((framenab_capture::crop(image, ox, oy, size.width, size.height), ox, oy))
}

#[tauri::command]
pub fn capture_info(app: AppHandle) -> Option<CaptureInfo> {
    let state = app.state::<CaptureState>();
    let mut guard = state.current.lock().expect("capture lock");
    let capture = guard.as_mut()?;
    if !capture.fitted {
        capture.fitted = true;
        if let Some((fitted, ox, oy)) = fit_to_monitor(&app, &capture.image) {
            capture.image = Arc::new(fitted);
            for w in &mut capture.windows {
                w.x -= ox;
                w.y -= oy;
            }
        }
    }
    Some(CaptureInfo {
        id: capture.id,
        width: capture.image.width(),
        height: capture.image.height(),
        windows: capture.windows.clone(),
        mode: *state.mode.lock().expect("mode lock"),
        window_picker: framenab_capture::Platform::detect().requires_portal(),
    })
}

/// Record mode: records `selection` (image pixels), or the whole screen if `None`.
#[tauri::command]
pub fn record_selection(app: AppHandle, selection: Option<Selection>) -> Result<(), String> {
    let (width, height) = {
        let state = app.state::<CaptureState>();
        let guard = state.current.lock().expect("capture lock");
        let capture = guard.as_ref().ok_or("no active screenshot")?;
        (capture.image.width(), capture.image.height())
    };
    let region = match selection {
        Some(s) => recorder::Region { x: s.x as i32, y: s.y as i32, w: s.width.max(2), h: s.height.max(2) },
        None => recorder::Region { x: 0, y: 0, w: width, h: height },
    };
    open_recorder(&app, Some(region));
    Ok(())
}

/// Record mode: records a single window chosen in the system dialog (Wayland).
#[tauri::command]
pub fn record_window(app: AppHandle) -> Result<(), String> {
    open_recorder(&app, None);
    Ok(())
}

fn open_recorder(app: &AppHandle, region: Option<recorder::Region>) {
    let image = {
        let state = app.state::<CaptureState>();
        let guard = state.current.lock().expect("capture lock");
        guard.as_ref().map(|c| Arc::clone(&c.image))
    };
    let size = image.as_ref().map_or((1, 1), |i| (i.width(), i.height()));
    // The recorder takes over bringing the main window back.
    let restore = app.state::<CaptureState>().restore.take();
    windows::close_overlay(app);
    recorder::open(app, region, size, image, restore);
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
    // Don't bring the main window back now (it would cover the new pin); hand that over to the
    // pin, which restores it when the last pin closes.
    let restore = app.state::<CaptureState>().restore.take();
    pin::open(&app, result, scale.max(0.1), restore).map_err(|e| e.to_string())?;
    windows::close_overlay(&app);
    Ok(())
}

#[tauri::command]
pub fn cancel_capture(app: AppHandle) {
    windows::close_overlay(&app);
}

/// Serves raw RGBA pixels: `framenab://localhost/<id>` for the current capture and
/// `framenab://localhost/pin/<id>` for pinned screenshots.
///
/// Runs off the main thread (registered as an asynchronous protocol), so decoding and encoding
/// images never blocks the UI.
pub fn protocol<R: Runtime>(app: &AppHandle<R>, request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let respond_as = |status: StatusCode, content_type: &str, body: Vec<u8>| {
        Response::builder()
            .status(status)
            .header("Content-Type", content_type)
            .header("Access-Control-Allow-Origin", "*")
            .body(Cow::Owned(body))
            .expect("valid response")
    };
    // `convertFileSrc` in the frontend percent-encodes the path, so `pin/3` arrives as `pin%2F3`.
    let decoded = request.uri().path().replace("%2F", "/").replace("%2f", "/");
    let path = decoded.trim_matches('/');
    let respond = |status: StatusCode, body: Vec<u8>| respond_as(status, "application/octet-stream", body);
    if let Some(file) = path.strip_prefix("asset/") {
        return match crate::editor::serve_asset(app, file) {
            Some(png) => respond_as(StatusCode::OK, "image/png", png),
            None => respond(StatusCode::NOT_FOUND, Vec::new()),
        };
    }
    if path.starts_with("thumb/") || path.starts_with("preview/") {
        return match crate::editor::serve(app, path) {
            Some(jpeg) => respond_as(StatusCode::OK, "image/jpeg", jpeg),
            None => respond(StatusCode::NOT_FOUND, Vec::new()),
        };
    }
    if let Some(pin_id) = path.strip_prefix("pin/").and_then(|id| id.parse().ok()) {
        return match app.state::<pin::PinState>().image(pin_id) {
            Some(image) => respond(StatusCode::OK, image.as_raw().clone()),
            None => respond(StatusCode::NOT_FOUND, Vec::new()),
        };
    }
    if path == "recorder-background" {
        return match app.state::<recorder::RecorderState>().background() {
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
