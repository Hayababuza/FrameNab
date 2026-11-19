//! Wayland backend via xdg-desktop-portal.
//!
//! The portal writes the screenshot to a PNG file and returns its URI. We load the file and then
//! delete it, so screenshots taken through FrameNapper do not pile up in the user's Pictures folder.

use std::path::PathBuf;
use std::sync::OnceLock;

use ashpd::desktop::screenshot::Screenshot;
use image::RgbaImage;

use crate::{CaptureBackend, CaptureError, CaptureTarget, FrameStream, MonitorInfo, Platform, Result, StreamOptions};

#[derive(Debug, Default, Clone, Copy)]
pub struct PortalBackend;

impl PortalBackend {
    /// Takes a non-interactive screenshot of the whole desktop through the portal.
    pub fn screenshot_all() -> Result<RgbaImage> {
        let uri = runtime()?.block_on(async {
            let request = Screenshot::request().interactive(false).modal(false).send().await.map_err(map_err)?;
            request.response().map(|r| r.uri().as_str().to_owned()).map_err(map_err)
        })?;

        let path = file_uri_to_path(&uri).ok_or_else(|| CaptureError::Backend(format!("unexpected URI {uri}")))?;
        let image = image::open(&path).map_err(|e| CaptureError::Backend(format!("{}: {e}", path.display())))?;
        if let Err(err) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), %err, "could not delete portal screenshot file");
        }
        Ok(image.into_rgba8())
    }
}

/// One runtime for the whole process. ashpd caches its D-Bus connection globally, and zbus runs
/// that connection on the runtime it was created on: a per-call runtime would leave the cached
/// connection without a driver, and every portal call after the first would hang forever.
pub(crate) fn runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<std::io::Result<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("framenapper-portal")
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|e| CaptureError::Backend(e.to_string()))
}

pub(crate) fn map_err(err: ashpd::Error) -> CaptureError {
    match err {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => CaptureError::PermissionDenied,
        ashpd::Error::Response(ashpd::desktop::ResponseError::Other) => CaptureError::AccessRequired,
        other => CaptureError::Backend(other.to_string()),
    }
}

/// Converts a `file://` URI into a path, decoding `%XX` escapes.
fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(out).ok().map(PathBuf::from)
    }
}

impl CaptureBackend for PortalBackend {
    fn platform(&self) -> Platform {
        Platform::LinuxWayland
    }

    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        // The portal does not expose monitor geometry; the app uses the windowing toolkit instead.
        Ok(Vec::new())
    }

    fn screenshot(&self, target: &CaptureTarget) -> Result<RgbaImage> {
        match target {
            CaptureTarget::AllMonitors => Self::screenshot_all(),
            CaptureTarget::Region { x, y, width, height } => {
                let all = Self::screenshot_all()?;
                Ok(crate::crop(&all, *x, *y, *width, *height))
            }
            CaptureTarget::Monitor(_) | CaptureTarget::Window(_) => Err(CaptureError::NotImplemented(self.platform())),
        }
    }

    fn open_stream(&self, target: &CaptureTarget, options: &StreamOptions) -> Result<Box<dyn FrameStream>> {
        let window = matches!(target, CaptureTarget::Window(_));
        Ok(Box::new(super::screencast::PortalStream::open(options, window)?))
    }
}

#[cfg(test)]
mod tests {
    use super::file_uri_to_path;
    use std::path::PathBuf;

    #[test]
    fn decodes_file_uris() {
        assert_eq!(
            file_uri_to_path("file:///home/me/Bilder/Bildschirmfoto%20vom%202026.png"),
            Some(PathBuf::from("/home/me/Bilder/Bildschirmfoto vom 2026.png"))
        );
        assert_eq!(file_uri_to_path("file:///a/%C3%A4.png"), Some(PathBuf::from("/a/ä.png")));
        assert_eq!(file_uri_to_path("https://example.com"), None);
    }
}
