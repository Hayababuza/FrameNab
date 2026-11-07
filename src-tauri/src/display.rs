//! Which window system FrameNab's own windows use on Linux.
//!
//! In a Wayland session, native Wayland windows may neither stay on top nor choose their
//! position: the recording frame and pins could not stay visible, and the frame could not be
//! placed exactly. X11 windows through XWayland may do both, so by default FrameNab runs its
//! windows on XWayland in Wayland sessions. Screen capture still goes through the portal.
//! The setting `nativeWayland` (settings.json) switches back; it applies after a restart.

use std::sync::atomic::{AtomicBool, Ordering};

use framenab_capture::Platform;

static XWAYLAND: AtomicBool = AtomicBool::new(false);

/// Settings key: `true` = use native Wayland windows instead of XWayland.
#[cfg(target_os = "linux")]
const NATIVE_WAYLAND_KEY: &str = "nativeWayland";

/// Chooses the GTK backend. Must run before GTK is initialized (first thing in `run`).
pub fn choose_backend() {
    #[cfg(target_os = "linux")]
    {
        let wayland_session = Platform::detect() == Platform::LinuxWayland;
        let x_available = std::env::var_os("DISPLAY").is_some();
        let forced = std::env::var_os("GDK_BACKEND").is_some();
        if wayland_session && x_available && !forced && !native_wayland_preferred() {
            gtk::gdk::set_allowed_backends("x11");
            XWAYLAND.store(true, Ordering::SeqCst);
        }
    }
}

/// FrameNab's windows run on XWayland inside a Wayland session.
pub fn xwayland() -> bool {
    XWAYLAND.load(Ordering::SeqCst)
}

/// FrameNab's windows are native Wayland windows: they cannot stay on top or be positioned.
pub fn native_wayland() -> bool {
    Platform::detect() == Platform::LinuxWayland && !xwayland()
}

/// Reads the setting directly from settings.json: the Tauri store is not available yet.
#[cfg(target_os = "linux")]
fn native_wayland_preferred() -> bool {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")));
    let Some(file) = data.map(|d| d.join("dev.framenab.app").join("settings.json")) else { return false };
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|v| v.get(NATIVE_WAYLAND_KEY)?.as_bool())
        .unwrap_or(false)
}
