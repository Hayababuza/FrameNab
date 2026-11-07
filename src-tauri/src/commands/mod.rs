//! Tauri commands the frontend calls via `invoke` (wrapped type-safely in `src/lib/ipc`).

use framenab_capture::Platform;
use framenab_export::ExportFormat;
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
    export_formats: Vec<ExportFormat>,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    let platform = Platform::detect();
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform,
        requires_portal: platform.requires_portal(),
        ffmpeg_available: framenab_export::find_ffmpeg().is_some(),
        export_formats: framenab_export::available_formats(),
    }
}

#[tauri::command]
pub fn run_action(app: AppHandle, window: WebviewWindow, action: Action) {
    // Triggered from the main window: hide it so it is not part of the screenshot.
    if window.label() == windows::MAIN {
        match action {
            Action::Screenshot => return crate::capture::start(&app, true, crate::capture::CaptureMode::Screenshot),
            Action::Record => return windows::record(&app, true),
            _ => {}
        }
    }
    windows::dispatch(&app, action);
}
