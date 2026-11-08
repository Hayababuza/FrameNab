//! Project export: GIF (gifski), APNG, WebP, PNG sequence and, if FFmpeg is installed,
//! MP4/WebM. FFmpeg is not bundled.
//!
//! Still images: [`still`]; recordings: [`animation`] (GIF, MP4, WebM so far).

pub mod animation;
mod formats;
pub mod still;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Gif,
    Apng,
    Webp,
    PngSequence,
    Mp4,
    Webm,
}

impl ExportFormat {
    pub const ALL: [Self; 6] = [Self::Gif, Self::Apng, Self::Webp, Self::PngSequence, Self::Mp4, Self::Webm];

    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Apng => "png",
            Self::Webp => "webp",
            Self::PngSequence => "zip",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    #[must_use]
    pub fn requires_ffmpeg(self) -> bool {
        matches!(self, Self::Mp4 | Self::Webm)
    }
}

/// Settings of an animation export (the export dialog).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportOptions {
    /// 1–100: GIF quality (gifski), lossy WebP quality, video quality.
    pub quality: u8,
    /// GIF: lossy compression (smaller, slight noise); WebP: lossy instead of lossless (FFmpeg).
    pub lossy: bool,
    /// Repetitions of GIF/APNG/WebP (0 = forever); `None` = the project's value.
    pub loop_count: Option<u16>,
    /// Output size relative to the project (0.1–1).
    pub scale: f32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { quality: 90, lossy: false, loop_count: None, scale: 1.0 }
    }
}

impl ExportOptions {
    /// Output size for a `width` × `height` project (at least 1 × 1).
    #[must_use]
    pub fn output_size(&self, width: u32, height: u32) -> (u32, u32) {
        let s = if self.scale.is_finite() { self.scale.clamp(0.05, 1.0) } else { 1.0 };
        (((width as f32 * s).round() as u32).max(1), ((height as f32 * s).round() as u32).max(1))
    }
}

/// Looks for an installed FFmpeg executable on `PATH`.
#[must_use]
pub fn find_ffmpeg() -> Option<PathBuf> {
    which::which("ffmpeg").ok()
}

/// Formats that can currently be exported on this system.
#[must_use]
pub fn available_formats() -> Vec<ExportFormat> {
    let ffmpeg = find_ffmpeg().is_some();
    ExportFormat::ALL.into_iter().filter(|f| ffmpeg || !f.requires_ffmpeg()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_ffmpeg_formats_always_available() {
        let formats = available_formats();
        for f in [ExportFormat::Gif, ExportFormat::Apng, ExportFormat::Webp, ExportFormat::PngSequence] {
            assert!(formats.contains(&f));
        }
    }

    #[test]
    fn format_serializes_camel_case() {
        assert_eq!(serde_json::to_string(&ExportFormat::PngSequence).unwrap(), "\"pngSequence\"");
    }
}
