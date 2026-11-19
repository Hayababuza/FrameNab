//! Global mouse clicks and key presses during a recording, for the click and key effects.
//!
//! * Linux (X11 and Wayland): read from the input devices (`/dev/input/event*`). That needs read
//!   access, i.e. membership in the `input` group; without it, nothing is recorded.
//! * Windows/macOS: polled with `device_query` (macOS asks for the Input Monitoring /
//!   Accessibility permission), which also provides the mouse position.
//!
//! Only presses are recorded, only while a recording runs, and only as short labels
//! (`Ctrl + C`); nothing is kept outside the project folder.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::InputRecorder;

/// Whether clicks and keys can be recorded on this system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InputSupport {
    Available,
    /// Linux: the input devices exist but are not readable (not in the `input` group).
    NoPermission,
    Unsupported,
}

/// Stops listening when dropped.
pub struct InputListener {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for InputListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn spawn(body: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> InputListener {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let thread = thread::Builder::new().name("framenapper-input".into()).spawn(move || body(flag)).ok();
    InputListener { stop, thread }
}

/// Polling interval of the listeners.
const POLL: Duration = Duration::from_millis(8);

/// Turns modifier state and a pressed key into a label like `Ctrl + Shift + T`.
fn label(modifiers: &[&str], key: &str) -> String {
    let mut parts: Vec<&str> =
        ["Ctrl", "Alt", "AltGr", "Shift", "Super"].into_iter().filter(|m| modifiers.contains(m)).collect();
    parts.push(key);
    parts.join(" + ")
}

/// `PAGEUP` → `Pageup`, single characters stay as they are.
fn title_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if name.len() > 1 => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        _ => name.to_owned(),
    }
}

// ---- Linux: evdev -------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod platform {
    use std::os::fd::OwnedFd;
    use std::os::unix::fs::OpenOptionsExt;

    use evdev::{Device, EventSummary, KeyCode};

    use super::{InputListener, InputRecorder, InputSupport, POLL, label, spawn, title_case};

    const O_NONBLOCK: i32 = 0o4000;

    fn event_paths() -> Vec<std::path::PathBuf> {
        std::fs::read_dir("/dev/input")
            .map(|dir| {
                dir.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("event")))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Opens the keyboards and mice, non-blocking (so the listener can stop at any time).
    fn open_devices() -> Vec<Device> {
        event_paths()
            .into_iter()
            .filter_map(|path| {
                let file = std::fs::OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(path).ok()?;
                let device = Device::from_fd(OwnedFd::from(file)).ok()?;
                let keys = device.supported_keys()?;
                (keys.contains(KeyCode::KEY_A) || keys.contains(KeyCode::BTN_LEFT)).then_some(device)
            })
            .collect()
    }

    pub fn support() -> InputSupport {
        let paths = event_paths();
        if paths.is_empty() {
            return InputSupport::Unsupported;
        }
        let readable =
            paths.iter().any(|p| std::fs::OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(p).is_ok());
        if readable { InputSupport::Available } else { InputSupport::NoPermission }
    }

    fn modifier(code: KeyCode) -> Option<&'static str> {
        Some(match code {
            KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => "Ctrl",
            KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => "Shift",
            KeyCode::KEY_LEFTALT => "Alt",
            KeyCode::KEY_RIGHTALT => "AltGr",
            KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => "Super",
            _ => return None,
        })
    }

    /// `KEY_A` → `A`, `KEY_PAGEUP` → `Pageup`. Names follow the US layout.
    pub(super) fn key_name(code: KeyCode) -> String {
        let debug = format!("{code:?}");
        title_case(debug.strip_prefix("KEY_").unwrap_or(&debug))
    }

    pub fn listen(recorder: InputRecorder) -> Option<InputListener> {
        let mut devices = open_devices();
        if devices.is_empty() {
            return None;
        }
        Some(spawn(move |stop| {
            let mut modifiers: Vec<&'static str> = Vec::new();
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                for device in &mut devices {
                    let Ok(events) = device.fetch_events() else { continue };
                    for event in events {
                        let EventSummary::Key(_, code, value) = event.destructure() else { continue };
                        let button = match code {
                            KeyCode::BTN_LEFT => Some(0),
                            KeyCode::BTN_RIGHT => Some(1),
                            KeyCode::BTN_MIDDLE => Some(2),
                            _ => None,
                        };
                        match (button, modifier(code), value) {
                            (Some(b), _, 1) => recorder.click(b),
                            (None, Some(m), 1) if !modifiers.contains(&m) => modifiers.push(m),
                            (None, Some(m), 0) => modifiers.retain(|x| *x != m),
                            (None, None, 1) => recorder.key(label(&modifiers, &key_name(code))),
                            _ => {}
                        }
                    }
                }
                std::thread::sleep(POLL);
            }
        }))
    }
}

// ---- Windows/macOS: device_query ----------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
mod platform {
    use device_query::{DeviceQuery, DeviceState, Keycode};

    use super::{InputListener, InputRecorder, InputSupport, POLL, label, spawn, title_case};

    pub fn support() -> InputSupport {
        InputSupport::Available
    }

    fn modifier(key: Keycode) -> Option<&'static str> {
        Some(match key {
            Keycode::LControl | Keycode::RControl => "Ctrl",
            Keycode::LShift | Keycode::RShift => "Shift",
            Keycode::LAlt => "Alt",
            Keycode::RAlt => "AltGr",
            Keycode::LMeta | Keycode::RMeta | Keycode::Command | Keycode::RCommand => "Super",
            _ => return None,
        })
    }

    pub fn listen(recorder: InputRecorder, origin: (i32, i32)) -> Option<InputListener> {
        Some(spawn(move |stop| {
            let state = DeviceState::new();
            let mut buttons: Vec<bool> = Vec::new();
            let mut keys: Vec<Keycode> = Vec::new();
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                let mouse = state.get_mouse();
                // Streams without cursor metadata: take the mouse position from the system.
                recorder.set_cursor(((mouse.coords.0 - origin.0) as f32, (mouse.coords.1 - origin.1) as f32));
                for (i, &pressed) in mouse.button_pressed.iter().enumerate() {
                    let was = buttons.get(i).copied().unwrap_or(false);
                    // device_query: 1 = left, 2 = right, 3 = middle.
                    if pressed && !was && (1..=3).contains(&i) {
                        recorder.click((i - 1) as u8);
                    }
                }
                buttons = mouse.button_pressed;

                let now = state.get_keys();
                let held: Vec<&'static str> = now.iter().filter_map(|k| modifier(*k)).collect();
                for key in now.iter().filter(|k| !keys.contains(k) && modifier(**k).is_none()) {
                    let name = format!("{key:?}");
                    recorder.key(label(&held, &title_case(name.strip_prefix("Key").unwrap_or(&name))));
                }
                keys = now;
                std::thread::sleep(POLL);
            }
        }))
    }
}

pub use platform::support;

/// Starts recording clicks and keys into `recorder`. `origin` is the stream's position on the
/// desktop (for the system mouse position on Windows/macOS). `None` if nothing can be recorded.
pub fn listen(recorder: InputRecorder, origin: (i32, i32)) -> Option<InputListener> {
    #[cfg(target_os = "linux")]
    {
        let _ = origin;
        platform::listen(recorder)
    }
    #[cfg(not(target_os = "linux"))]
    {
        platform::listen(recorder, origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_put_modifiers_first_in_a_fixed_order() {
        assert_eq!(label(&["Shift", "Ctrl"], "T"), "Ctrl + Shift + T");
        assert_eq!(label(&[], "Enter"), "Enter");
        assert_eq!(title_case("PAGEUP"), "Pageup");
        assert_eq!(title_case("A"), "A");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn evdev_key_names() {
        assert_eq!(platform::key_name(evdev::KeyCode::KEY_A), "A");
        assert_eq!(platform::key_name(evdev::KeyCode::KEY_ENTER), "Enter");
        assert_eq!(platform::key_name(evdev::KeyCode::KEY_1), "1");
    }
}
