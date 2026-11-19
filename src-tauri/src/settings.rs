//! Settings stored by the frontend in `settings.json` (tauri-plugin-store), read from Rust.

use std::path::PathBuf;

use framenapper_export::still::{DEFAULT_TEMPLATE, StillFormat};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

pub struct SaveSettings {
    pub directory: PathBuf,
    pub template: String,
    pub format: StillFormat,
}

/// `<Pictures>/FrameNapper`, or `<Home>/FrameNapper` if there is no pictures folder.
pub fn default_save_dir(app: &AppHandle) -> PathBuf {
    let path = app.path();
    path.picture_dir().or_else(|_| path.home_dir()).unwrap_or_else(|_| PathBuf::from(".")).join("FrameNapper")
}

pub fn save_settings(app: &AppHandle) -> SaveSettings {
    let store = app.store("settings.json").ok();
    let get = |key: &str| store.as_ref().and_then(|s| s.get(key));
    let directory = get("saveDirectory")
        .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(PathBuf::from))
        .unwrap_or_else(|| default_save_dir(app));
    let template = get("filenameTemplate")
        .and_then(|v| v.as_str().filter(|s| !s.trim().is_empty()).map(str::to_owned))
        .unwrap_or_else(|| DEFAULT_TEMPLATE.to_owned());
    let format = get("imageFormat").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    SaveSettings { directory, template, format }
}

#[tauri::command]
pub fn default_save_directory(app: AppHandle) -> String {
    default_save_dir(&app).display().to_string()
}
