//! Screen capture behind a common trait.
//!
//! | Platform      | Screenshot              | Recording                  |
//! |---------------|-------------------------|----------------------------|
//! | Windows       | DXGI / GDI              | Windows.Graphics.Capture   |
//! | macOS         | ScreenCaptureKit        | ScreenCaptureKit           |
//! | Linux X11     | XShm                    | XShm in a loop             |
//! | Linux Wayland | xdg-desktop-portal      | Portal ScreenCast+PipeWire |
//!
//! Phase 0: trait, types and platform detection. The backends follow in phase 1/2.

use image::RgbaImage;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("This capture backend is not implemented on {0} yet")]
    NotImplemented(Platform),
    #[error("The user denied screen sharing")]
    PermissionDenied,
    #[error("Monitor {0} not found")]
    MonitorNotFound(u32),
    #[error("Capture error: {0}")]
    Backend(String),
}

pub type Result<T, E = CaptureError> = std::result::Result<T, E>;

/// Which platform / display server FrameNab is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Windows,
    MacOs,
    LinuxX11,
    LinuxWayland,
    Unknown,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::LinuxX11 => "Linux (X11)",
            Self::LinuxWayland => "Linux (Wayland)",
            Self::Unknown => "an unknown platform",
        })
    }
}

impl Platform {
    /// Detects the current platform at runtime.
    #[must_use]
    pub fn detect() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            Self::detect_linux(
                std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
                std::env::var_os("WAYLAND_DISPLAY").is_some(),
            )
        } else {
            Self::Unknown
        }
    }

    fn detect_linux(session_type: Option<&str>, wayland_display: bool) -> Self {
        match session_type {
            Some("wayland") => Self::LinuxWayland,
            Some("x11") => Self::LinuxX11,
            _ if wayland_display => Self::LinuxWayland,
            _ => Self::LinuxX11,
        }
    }

    /// On Wayland, apps may neither read the screen freely nor register global shortcuts
    /// themselves; both are only possible through xdg-desktop-portal.
    #[must_use]
    pub fn requires_portal(self) -> bool {
        self == Self::LinuxWayland
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    /// Position and size in physical pixels within the virtual desktop.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub is_primary: bool,
}

/// What to capture. Coordinates are in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureTarget {
    /// All monitors as a single image (basis for the screenshot overlay).
    AllMonitors,
    Monitor(u32),
    Region {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Window(u64),
}

/// Receives frames during a recording. `timestamp_ms` counts from the start of the recording.
pub trait FrameSink: Send {
    fn push(&mut self, image: RgbaImage, timestamp_ms: u64);
}

/// A running recording. Dropping it ends the recording.
pub trait RecordingHandle: Send {
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(self: Box<Self>) -> Result<()>;
}

pub trait CaptureBackend: Send + Sync {
    fn platform(&self) -> Platform;
    fn monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn screenshot(&self, target: &CaptureTarget) -> Result<RgbaImage>;
    fn start_recording(
        &self,
        target: &CaptureTarget,
        fps: u32,
        sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn RecordingHandle>>;
}

/// Placeholder backend until the real backends are implemented.
#[derive(Debug, Clone, Copy)]
pub struct UnimplementedBackend(Platform);

impl CaptureBackend for UnimplementedBackend {
    fn platform(&self) -> Platform {
        self.0
    }
    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        Err(CaptureError::NotImplemented(self.0))
    }
    fn screenshot(&self, _: &CaptureTarget) -> Result<RgbaImage> {
        Err(CaptureError::NotImplemented(self.0))
    }
    fn start_recording(&self, _: &CaptureTarget, _: u32, _: Box<dyn FrameSink>) -> Result<Box<dyn RecordingHandle>> {
        Err(CaptureError::NotImplemented(self.0))
    }
}

/// Returns the matching backend for the current platform.
#[must_use]
pub fn backend() -> Box<dyn CaptureBackend> {
    Box::new(UnimplementedBackend(Platform::detect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_session_detection() {
        assert_eq!(Platform::detect_linux(Some("wayland"), false), Platform::LinuxWayland);
        assert_eq!(Platform::detect_linux(Some("x11"), true), Platform::LinuxX11);
        assert_eq!(Platform::detect_linux(None, true), Platform::LinuxWayland);
        assert_eq!(Platform::detect_linux(Some("tty"), false), Platform::LinuxX11);
    }

    #[test]
    fn only_wayland_needs_portal() {
        assert!(Platform::LinuxWayland.requires_portal());
        assert!(!Platform::Windows.requires_portal());
    }
}
