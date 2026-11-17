//! Global shortcuts, configurable in the settings (`hotkeys` in `settings.json`, accelerators
//! like `Shift+PrintScreen`). Not possible on Wayland (see `docs/wayland.md`); there the system
//! owns the shortcut and runs `framenab gui` / `framenab record`.

use std::str::FromStr;
use std::sync::Mutex;

use framenab_capture::Platform;
use serde::{Deserialize, Serialize};
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_store::StoreExt;

use crate::cli::Action;
use crate::windows;

/// Settings key.
const KEY: &str = "hotkeys";

/// The shortcuts as stored in the settings; `None` = switched off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    pub screenshot: Option<String>,
    pub record: Option<String>,
    /// Start recording the last area right away.
    pub record_now: Option<String>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            screenshot: Some("PrintScreen".into()),
            record: Some("Shift+PrintScreen".into()),
            record_now: Some("Ctrl+Shift+PrintScreen".into()),
        }
    }
}

/// The registered shortcuts and what they do.
#[derive(Default)]
pub struct HotkeyState(Mutex<Vec<(Shortcut, Action)>>);

/// A shortcut that could not be registered.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyProblem {
    /// `screenshot`, `record` or `recordNow`.
    pub action: &'static str,
    pub shortcut: String,
    /// Invalid, or already taken by another program.
    pub error: String,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let action = app
                .state::<HotkeyState>()
                .0
                .lock()
                .expect("hotkey lock")
                .iter()
                .find(|(s, _)| s == shortcut)
                .map(|(_, a)| *a);
            if let Some(action) = action {
                windows::dispatch(app, action);
            }
        })
        .build()
}

fn load(app: &AppHandle) -> Hotkeys {
    app.store("settings.json")
        .ok()
        .and_then(|s| s.get(KEY))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Registers the shortcuts from the settings (replacing the previous ones).
pub fn register(app: &AppHandle) -> Vec<HotkeyProblem> {
    if Platform::detect().requires_portal() {
        tracing::info!("Wayland detected: bind a system shortcut to `framenab gui` instead");
        return Vec::new();
    }
    let hotkeys = load(app);
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let mut registered = Vec::new();
    let mut problems = Vec::new();
    for (name, action, accelerator) in [
        ("screenshot", Action::Screenshot, hotkeys.screenshot),
        ("record", Action::Record, hotkeys.record),
        ("recordNow", Action::RecordNow, hotkeys.record_now),
    ] {
        let Some(accelerator) = accelerator.filter(|a| !a.trim().is_empty()) else { continue };
        let result = Shortcut::from_str(&accelerator)
            .map_err(|e| e.to_string())
            .and_then(|s| shortcuts.register(s).map(|()| s).map_err(|e| e.to_string()));
        match result {
            Ok(shortcut) => registered.push((shortcut, action)),
            Err(error) => {
                tracing::warn!(%accelerator, %error, "shortcut not registered");
                problems.push(HotkeyProblem { action: name, shortcut: accelerator, error });
            }
        }
    }
    *app.state::<HotkeyState>().0.lock().expect("hotkey lock") = registered;
    problems
}

/// Applies the shortcuts saved in the settings; returns the ones that did not work.
#[tauri::command]
pub fn apply_hotkeys(app: AppHandle) -> Vec<HotkeyProblem> {
    register(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_stored_values_parse() {
        let defaults = Hotkeys::default();
        for accelerator in [
            defaults.screenshot.unwrap(),
            defaults.record.unwrap(),
            defaults.record_now.unwrap(),
            "Ctrl+Alt+KeyA".into(),
        ] {
            assert!(Shortcut::from_str(&accelerator).is_ok(), "{accelerator}");
        }
        let stored: Hotkeys = serde_json::from_str(r#"{"screenshot":null}"#).unwrap();
        assert_eq!(
            stored,
            Hotkeys {
                screenshot: None,
                record: Some("Shift+PrintScreen".into()),
                record_now: Some("Ctrl+Shift+PrintScreen".into())
            }
        );
    }
}
