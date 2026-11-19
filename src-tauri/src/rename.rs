//! Takes over what an installation from before the rename (the app used to be called "FrameNab")
//! left behind: the app folders (settings, recordings, logs), the default file name template and
//! the autostart entry.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

/// The app's identifier before the rename (the name of its folders).
const OLD_IDENTIFIER: &str = "dev.framenab.app";
/// The app's identifier now; must match `identifier` in `tauri.conf.json` (checked by a test).
pub const IDENTIFIER: &str = "dev.framenapper.app";
/// The app name before the rename (autostart entries are named after it).
const OLD_NAME: &str = "FrameNab";
/// The default file name template before the rename.
const OLD_TEMPLATE: &str = "FrameNab_{yyyy-MM-dd_HH-mm-ss}";
/// Folder suffix of recordings made before the rename (new ones end in `.framenapper`).
pub const OLD_PROJECT_EXTENSION: &str = "framenab";

/// Moves the old app folders to the new names. Runs first thing in `main`, before anything
/// (WebView, settings store, logging) creates the new ones. Returns what it did, for the log.
pub fn migrate_folders() -> Vec<String> {
    let mut bases: Vec<PathBuf> = [dirs::data_dir(), dirs::data_local_dir(), dirs::config_dir(), dirs::cache_dir()]
        .into_iter()
        .flatten()
        .collect();
    bases.dedup();
    let mut done = Vec::new();
    for base in bases {
        match move_folder(&base.join(OLD_IDENTIFIER), &base.join(IDENTIFIER)) {
            Ok(true) => done.push(format!("moved {} to {IDENTIFIER}", base.join(OLD_IDENTIFIER).display())),
            Ok(false) => {}
            Err(err) => done.push(format!("could not move {}: {err}", base.join(OLD_IDENTIFIER).display())),
        }
    }
    if let Some(data) = dirs::data_dir()
        && update_template(&data.join(IDENTIFIER).join("settings.json"))
    {
        done.push("file name template: FrameNab_… is now FrameNapper_…".to_owned());
    }
    done
}

/// Moves `old` to `new` unless `new` exists already (then the old folder stays as it is).
fn move_folder(old: &Path, new: &Path) -> std::io::Result<bool> {
    if !old.is_dir() || new.exists() {
        return Ok(false);
    }
    std::fs::rename(old, new)?;
    Ok(true)
}

/// Replaces the old default file name template in `settings.json`; true if it changed.
fn update_template(settings: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(settings) else { return false };
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
    let Some(template) = json.get_mut("filenameTemplate") else { return false };
    if template.as_str() != Some(OLD_TEMPLATE) {
        return false;
    }
    *template = serde_json::Value::from(framenapper_export::still::DEFAULT_TEMPLATE);
    serde_json::to_string_pretty(&json).is_ok_and(|text| std::fs::write(settings, text).is_ok())
}

/// Moves the autostart entry of the old name over to the new one.
pub fn migrate_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;

    let Ok(exe) = std::env::current_exe() else { return };
    let mut builder = auto_launch::AutoLaunchBuilder::new();
    builder.set_app_name(OLD_NAME).set_app_path(&exe.display().to_string()).set_args(&["--hidden"]);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    let Ok(old) = builder.build() else { return };
    if !old.is_enabled().unwrap_or(false) {
        return;
    }
    if let Err(err) = old.disable() {
        tracing::warn!(%err, "could not remove the old FrameNab autostart entry");
        return;
    }
    match app.autolaunch().enable() {
        Ok(()) => tracing::info!("autostart moved over from FrameNab"),
        Err(err) => tracing::warn!(%err, "could not enable autostart"),
    }
}

/// A recording folder: `*.framenapper`, or `*.framenab` from before the rename.
pub fn is_project_extension(extension: &std::ffi::OsStr) -> bool {
    extension == "framenapper" || extension == OLD_PROJECT_EXTENSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_matches_the_config() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
    }

    #[test]
    fn moves_only_when_the_new_folder_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("old"), dir.path().join("new"));
        assert!(!move_folder(&old, &new).unwrap(), "nothing to move");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("settings.json"), "{}").unwrap();
        assert!(move_folder(&old, &new).unwrap());
        assert!(new.join("settings.json").exists() && !old.exists());
        std::fs::create_dir(&old).unwrap();
        assert!(!move_folder(&old, &new).unwrap(), "an existing new folder is kept");
        assert!(old.exists());
    }

    #[test]
    fn replaces_only_the_old_default_template() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, r#"{"filenameTemplate": "FrameNab_{yyyy-MM-dd_HH-mm-ss}", "fps": 15}"#).unwrap();
        assert!(update_template(&file));
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(json["filenameTemplate"], framenapper_export::still::DEFAULT_TEMPLATE);
        assert_eq!(json["fps"], 15);
        std::fs::write(&file, r#"{"filenameTemplate": "Shot_{HH-mm}"}"#).unwrap();
        assert!(!update_template(&file), "a template of the user's own stays");
    }
}
