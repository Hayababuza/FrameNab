//! Bildschirmaufnahme hinter einem gemeinsamen Trait.
//!
//! | Plattform     | Screenshot              | Aufnahme                   |
//! |---------------|-------------------------|----------------------------|
//! | Windows       | DXGI / GDI              | Windows.Graphics.Capture   |
//! | macOS         | ScreenCaptureKit        | ScreenCaptureKit           |
//! | Linux X11     | XShm                    | XShm im Loop               |
//! | Linux Wayland | xdg-desktop-portal      | Portal ScreenCast+PipeWire |
//!
//! Phase 0: Trait, Typen und Plattform-Erkennung. Die Backends folgen in Phase 1/2.

use image::RgbaImage;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Dieses Capture-Backend ist auf {0} noch nicht implementiert")]
    NotImplemented(Platform),
    #[error("Der Nutzer hat die Bildschirmfreigabe abgelehnt")]
    PermissionDenied,
    #[error("Monitor {0} nicht gefunden")]
    MonitorNotFound(u32),
    #[error("Capture-Fehler: {0}")]
    Backend(String),
}

pub type Result<T, E = CaptureError> = std::result::Result<T, E>;

/// Auf welcher Plattform bzw. welchem Display-Server läuft FrameNab?
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
            Self::Unknown => "unbekannter Plattform",
        })
    }
}

impl Platform {
    /// Erkennt die aktuelle Plattform zur Laufzeit.
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

    /// Unter Wayland dürfen Apps weder frei den Bildschirm lesen noch selbst globale
    /// Tastenkürzel registrieren; beides geht nur über xdg-desktop-portal.
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
    /// Position und Größe in physischen Pixeln im virtuellen Desktop.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub is_primary: bool,
}

/// Was aufgenommen werden soll. Koordinaten in physischen Pixeln.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureTarget {
    /// Alle Monitore als ein Bild (Grundlage für das Screenshot-Overlay).
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

/// Empfängt Frames während einer Aufnahme. `timestamp_ms` zählt ab Aufnahmestart.
pub trait FrameSink: Send {
    fn push(&mut self, image: RgbaImage, timestamp_ms: u64);
}

/// Laufende Aufnahme. Wird sie gedroppt, endet die Aufnahme.
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

/// Platzhalter-Backend, bis die echten Backends implementiert sind.
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

/// Liefert das passende Backend für die aktuelle Plattform.
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
