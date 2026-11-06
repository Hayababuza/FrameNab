//! Tauri commands the frontend calls via `invoke` (wrapped type-safely in `src/lib/ipc`).

use framenab_capture::Platform;
use framenab_export::ExportFormat;
use serde::Serialize;
use tauri::AppHandle;

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
pub fn run_action(app: AppHandle, action: Action) {
    windows::dispatch(&app, action);
}
