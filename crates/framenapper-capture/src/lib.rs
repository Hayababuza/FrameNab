//! Screen capture behind a common trait.
//!
//! | Platform      | Screenshot              | Recording                  |
//! |---------------|-------------------------|----------------------------|
//! | Windows       | DXGI / GDI              | Windows.Graphics.Capture   |
//! | macOS         | ScreenCaptureKit        | ScreenCaptureKit           |
//! | Linux X11     | XShm                    | XShm in a loop             |
//! | Linux Wayland | xdg-desktop-portal      | Portal ScreenCast+PipeWire |
//!
//! Screenshots and recording work on every platform (see [`recording`]).

pub mod backends;
pub mod input;
pub mod recording;

pub use recording::{
    Crop, CursorImage, FrameSink, FrameStream, InputRecorder, Recording, SamplerStats, StreamOptions, record,
};

use image::RgbaImage;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("This capture backend is not implemented on {0} yet")]
    NotImplemented(Platform),
    #[error("The user denied screen sharing")]
    PermissionDenied,
    /// The portal refused without asking. GNOME only lets the focused app show its access
    /// dialog ("Allow … to take screenshots?"), so the request has to be repeated while a
    /// FrameNapper window has the focus.
    #[error("The system did not allow the screenshot")]
    AccessRequired,
    #[error("Monitor {0} not found")]
    MonitorNotFound(u32),
    #[error("Capture error: {0}")]
    Backend(String),
}

pub type Result<T, E = CaptureError> = std::result::Result<T, E>;

/// Which platform / display server FrameNapper is running on.
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

/// A top-level window on screen, in physical pixels within the virtual desktop.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub title: String,
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

pub trait CaptureBackend: Send + Sync {
    fn platform(&self) -> Platform;
    fn monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn screenshot(&self, target: &CaptureTarget) -> Result<RgbaImage>;
    /// Visible top-level windows, topmost first, for window snapping in the overlay. Platforms
    /// that do not expose other apps' windows (Wayland) return an empty list.
    fn windows(&self) -> Result<Vec<WindowInfo>> {
        Ok(Vec::new())
    }
    /// Opens a live stream of the monitor that contains `target` (on Wayland the user picks
    /// the monitor in the screen-sharing dialog). Use [`recording::record`] to sample it.
    fn open_stream(&self, target: &CaptureTarget, options: &StreamOptions) -> Result<Box<dyn FrameStream>>;
}

/// Returns the matching backend for the current platform.
#[must_use]
pub fn backend() -> Box<dyn CaptureBackend> {
    let platform = Platform::detect();
    match platform {
        #[cfg(target_os = "linux")]
        Platform::LinuxWayland => Box::new(backends::portal::PortalBackend),
        _ => Box::new(backends::xcap::XcapBackend(platform)),
    }
}

/// Bounding box `(min_x, min_y, width, height)` of all monitors in the virtual desktop.
#[must_use]
pub fn bounds(monitors: &[MonitorInfo]) -> Option<(i32, i32, u32, u32)> {
    let min_x = monitors.iter().map(|m| m.x).min()?;
    let min_y = monitors.iter().map(|m| m.y).min()?;
    let max_x = monitors.iter().map(|m| m.x + m.width as i32).max()?;
    let max_y = monitors.iter().map(|m| m.y + m.height as i32).max()?;
    Some((min_x, min_y, (max_x - min_x) as u32, (max_y - min_y) as u32))
}

/// Crops `image` to the given rectangle, clamped to the image bounds.
#[must_use]
pub fn crop(image: &RgbaImage, x: i32, y: i32, width: u32, height: u32) -> RgbaImage {
    let x0 = x.clamp(0, image.width() as i32) as u32;
    let y0 = y.clamp(0, image.height() as i32) as u32;
    let w = width.min(image.width() - x0);
    let h = height.min(image.height() - y0);
    image::imageops::crop_imm(image, x0, y0, w, h).to_image()
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
    fn crop_is_clamped() {
        let img = RgbaImage::new(10, 10);
        assert_eq!(crop(&img, 8, -3, 5, 5).dimensions(), (2, 5));
        assert_eq!(crop(&img, 20, 0, 5, 5).dimensions(), (0, 5));
    }

    #[test]
    fn monitor_bounds() {
        let m = |x, y, w, h| MonitorInfo {
            id: 0,
            name: String::new(),
            x,
            y,
            width: w,
            height: h,
            scale_factor: 1.0,
            is_primary: false,
        };
        assert_eq!(bounds(&[m(0, 0, 1920, 1080), m(-1280, 200, 1280, 1024)]), Some((-1280, 0, 3200, 1224)));
        assert_eq!(bounds(&[]), None);
    }

    #[test]
    fn only_wayland_needs_portal() {
        assert!(Platform::LinuxWayland.requires_portal());
        assert!(!Platform::Windows.requires_portal());
    }
}
