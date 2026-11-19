//! Tauri commands the frontend calls via `invoke` (wrapped type-safely in `src/lib/ipc`).

use framenapper_capture::Platform;
use framenapper_export::ExportFormat;
use serde::Serialize;
use tauri::{AppHandle, WebviewWindow};

use crate::cli::Action;
use crate::windows;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    platform: Platform,
    requires_portal: bool,
    ffmpeg_available: bool,
    /// Wayland session, FrameNapper windows running on XWayland.
    xwayland: bool,
    export_formats: Vec<ExportFormat>,
}

/// Brings the calling window to the front (see [`windows::focus`]).
#[tauri::command]
pub fn focus_window(window: WebviewWindow) {
    windows::focus(&window);
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    let platform = Platform::detect();
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform,
        requires_portal: platform.requires_portal(),
        ffmpeg_available: framenapper_export::find_ffmpeg().is_some(),
        xwayland: crate::display::xwayland(),
        export_formats: framenapper_export::available_formats(),
    }
}

#[tauri::command]
// `async`: it may open windows, which deadlocks on Windows from a synchronous command (wry#583).
pub async fn run_action(app: AppHandle, window: WebviewWindow, action: Action) {
    // Triggered from the main window: hide it so it is not part of the screenshot.
    // From the main window or the editor: hide it so it is not in the picture, restore it after.
    let label = window.label();
    if label == windows::MAIN || label == crate::editor::LABEL {
        let hide = Some(label.to_owned());
        match action {
            Action::Screenshot => return crate::capture::start(&app, hide, crate::capture::CaptureMode::Screenshot),
            Action::Record => return windows::record(&app, hide),
            Action::RecordNow => return windows::instant_gif(&app, hide),
            _ => {}
        }
    }
    windows::run(&app, action);
}
