//! Global shortcuts. Not possible on Wayland (see `docs/wayland.md`); there the system owns the
//! shortcut and runs `framenab gui`. Phase 5: configurable shortcuts and xdg-desktop-portal
//! GlobalShortcuts.

use framenab_capture::Platform;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::cli::Action;
use crate::windows;

fn screenshot() -> Shortcut {
    Shortcut::new(None, Code::PrintScreen)
}

fn record() -> Shortcut {
    Shortcut::new(Some(Modifiers::SHIFT), Code::PrintScreen)
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if shortcut == &screenshot() {
                windows::dispatch(app, Action::Screenshot);
            } else if shortcut == &record() {
                windows::dispatch(app, Action::Record);
            }
        })
        .build()
}

pub fn register_defaults(app: &AppHandle) {
    if Platform::detect().requires_portal() {
        tracing::info!("Wayland detected: bind a system shortcut to `framenab gui` instead");
        return;
    }
    for shortcut in [screenshot(), record()] {
        if let Err(err) = app.global_shortcut().register(shortcut) {
            tracing::warn!(%shortcut, %err, "Shortcut already taken or unavailable");
        }
    }
}
