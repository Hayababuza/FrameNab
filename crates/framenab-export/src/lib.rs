//! Project export: GIF (gifski), APNG, WebP, PNG sequence and, if FFmpeg is installed,
//! MP4/WebM. FFmpeg is not bundled.
//!
//! Phase 0: formats, options and FFmpeg detection. The encoders follow in phase 4.

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GifOptions {
    /// 1–100, passed through to gifski.
    pub quality: u8,
    /// Lossy compression for smaller files (gifski `lossy_quality`).
    pub lossy: Option<u8>,
    /// Fixed repetitions; `None` = use the project's value.
    pub loop_count: Option<u16>,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self { quality: 90, lossy: None, loop_count: None }
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
